mod build_identity;
mod file_system_handler;
mod handler;
mod process_handler;
mod processor;
mod registry;
mod release_version;
mod request_dispatcher;
mod session_registry;
mod transport;

#[cfg(all(test, unix))]
#[path = "server/process_otel_tests.rs"]
mod process_otel_tests;

pub(crate) use handler::ExecServerHandler;
pub(crate) use processor::ConnectionProcessor;
pub use request_dispatcher::ConcurrentRequestLimit;
pub use request_dispatcher::RequestDispatchMode;
pub use transport::DEFAULT_LISTEN_URL;
pub use transport::ExecServerListenUrlParseError;

use crate::ExecServerRuntimeOptions;
use crate::ExecServerTelemetry;
use codex_http_client::HttpClientFactory;
use codex_websocket_auth::WebsocketAuthSettings;

pub async fn run_main(
    listen_url: &str,
    runtime_paths: ExecServerRuntimeOptions,
    http_client_factory: HttpClientFactory,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_main_with_telemetry(
        listen_url,
        runtime_paths,
        ExecServerTelemetry::default(),
        http_client_factory,
        RequestDispatchMode::Inline,
        WebsocketAuthSettings::default(),
    )
    .await
}

#[tracing::instrument(
    name = "codex.exec_server",
    skip_all,
    fields(otel.kind = "internal")
)]
pub async fn run_main_with_telemetry(
    listen_url: &str,
    runtime_paths: ExecServerRuntimeOptions,
    telemetry: ExecServerTelemetry,
    http_client_factory: HttpClientFactory,
    request_dispatch_mode: RequestDispatchMode,
    websocket_auth: WebsocketAuthSettings,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if runtime_paths.execution_account.is_some() && !matches!(listen_url, "stdio" | "stdio://") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "dedicated account executor requires private stdio transport",
        )
        .into());
    }
    std::sync::LazyLock::force(&build_identity::PROVIDER_ID);
    transport::run_transport(
        listen_url,
        runtime_paths,
        telemetry,
        http_client_factory,
        request_dispatch_mode,
        websocket_auth,
    )
    .await
}

#[cfg(test)]
mod tests {
    use codex_http_client::HttpClientFactory;
    use codex_http_client::OutboundProxyPolicy;
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::trace::InMemorySpanExporter;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing::instrument::WithSubscriber;
    use tracing_subscriber::prelude::*;

    use super::run_main_with_telemetry;
    use crate::ExecServerRuntimeOptions;
    use crate::ExecServerTelemetry;

    #[tokio::test]
    async fn execution_account_rejects_network_listeners_before_binding() {
        let mut runtime =
            ExecServerRuntimeOptions::new(std::env::current_exe().unwrap(), None).unwrap();
        runtime.execution_account = Some(
            serde_json::from_value(serde_json::json!({
                "schema_version": 1, "username": "ci-user", "uid": 65001, "gid": 20,
                "home": "/Users/ci-user", "root": "/Users/ci-user/ci", "path": "/usr/bin:/bin"
            }))
            .unwrap(),
        );
        for listener in ["ws://127.0.0.1:0", "ws://0.0.0.0:0"] {
            let error = super::run_main(
                listener,
                runtime.clone(),
                HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains("private stdio"));
        }
    }

    #[tokio::test]
    async fn telemetry_entrypoint_emits_root_span() {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("exec-server-test")));

        async {
            tracing::callsite::rebuild_interest_cache();
            run_main_with_telemetry(
                "invalid",
                ExecServerRuntimeOptions::new(
                    std::env::current_exe().expect("current executable"),
                    /*codex_linux_sandbox_exe*/ None,
                )
                .expect("runtime paths"),
                ExecServerTelemetry::default(),
                HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
                super::RequestDispatchMode::Inline,
                codex_websocket_auth::WebsocketAuthSettings::default(),
            )
            .await
            .expect_err("invalid listen URL should fail");
        }
        .with_subscriber(subscriber)
        .await;

        provider.force_flush().expect("flush traces");
        let spans = exporter.get_finished_spans().expect("span export");
        assert!(
            spans.iter().any(|span| span.name == "codex.exec_server"),
            "root exec-server span missing: {spans:?}"
        );
    }
}
