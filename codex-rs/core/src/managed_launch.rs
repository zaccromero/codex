//! AGT-21's owner-installed, one-command route to the local managed proxy.
//!
//! The route is deliberately absent from remote `ExecParams`: those fields are
//! client supplied and do not carry the native owner's one-time decision.

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Arc;

use codex_exec_server::ManagedLaunchBinding;
use codex_exec_server::ManagedLaunchClaims;
use codex_network_proxy::NetworkProxy;
use codex_sandboxing::SandboxType;
use sha2::Digest;
use sha2::Sha256;
use url::Url;

pub(crate) const HELPER: &str = "/usr/local/libexec/drako/agt21-pr417-v2/materialize";
pub(crate) const ISSUER: &str = "/usr/local/libexec/drako/codex-fork-agt21-v2/bin/codex";
const DESTINATION: &str = "https://github.com/zaccromero/drako-os.git";
const GIT_REF: &str = "refs/pull/417/head";

pub(crate) struct PreparedManagedLaunch {
    pub(crate) command: Vec<String>,
    pub(crate) binding: Arc<ManagedLaunchBinding>,
}

#[expect(clippy::too_many_arguments)]
pub(crate) async fn prepare(
    raw_command: &str,
    use_login_shell: bool,
    tty: bool,
    human_approved: bool,
    sandbox: SandboxType,
    network: Option<&NetworkProxy>,
    network_environment_id: &str,
    child_env: &HashMap<String, String>,
) -> Result<Option<PreparedManagedLaunch>, String> {
    if !raw_command.starts_with(HELPER) {
        return Ok(None);
    }
    if !human_approved || use_login_shell || tty || sandbox != SandboxType::MacosSeatbelt {
        return Err("managed helper requires its exact native approved, sandboxed launch".into());
    }
    let head = parse_head(raw_command, HELPER)
        .ok_or_else(|| "managed helper requires exact --head SHA invocation".to_string())?;
    let network = network.ok_or_else(|| "managed helper requires the live proxy".to_string())?;
    if !network
        .permits_destination("github.com", 443)
        .await
        .map_err(|_| "managed destination policy unavailable".to_string())?
    {
        return Err("managed destination is not permitted".into());
    }
    let prepared = network
        .prepare_for_optional_environment(HashMap::new(), Some(network_environment_id))
        .map_err(|_| "managed proxy environment unavailable".to_string())?;
    let proxy = prepared
        .env
        .get("HTTPS_PROXY")
        .ok_or_else(|| "managed HTTPS proxy unavailable".to_string())?;
    if child_env.get("HTTPS_PROXY") != Some(proxy) || !credential_free_loopback_http(proxy) {
        return Err("managed HTTPS transport differs from proxy issuer".into());
    }
    let issuer = Path::new(ISSUER);
    if std::env::current_exe().map_err(|_| "issuer image unavailable")? != issuer
        || !root_sealed(issuer, false)
        || !root_sealed(Path::new(HELPER), false)
    {
        return Err("owner-installed issuer or helper is not sealed".into());
    }
    let helper_bytes = fs::read(HELPER).map_err(|_| "sealed helper unavailable")?;
    let command = vec![HELPER.to_string(), "--head".to_string(), head.to_string()];
    let binding = ManagedLaunchBinding::new(ManagedLaunchClaims {
        helper: HELPER.to_string(),
        helper_sha256: format!("{:x}", Sha256::digest(&helper_bytes)),
        argv: command.clone(),
        https_proxy: proxy.clone(),
        destination: DESTINATION.to_string(),
        git_ref: GIT_REF.to_string(),
        issuer_exe: ISSUER.to_string(),
    })
    .map_err(|_| "managed launch channel unavailable")?;
    Ok(Some(PreparedManagedLaunch { command, binding }))
}

fn parse_head<'a>(command: &'a str, helper: &str) -> Option<&'a str> {
    let head = command.strip_prefix(helper)?.strip_prefix(" --head ")?;
    (head.len() == 40
        && head
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some(head)
}

fn credential_free_loopback_http(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "::1"))
        && url.port().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
}

fn root_sealed(path: &Path, directory: bool) -> bool {
    if !path.is_absolute() {
        return false;
    }
    let mut first = true;
    for part in path.ancestors() {
        let Ok(meta) = fs::symlink_metadata(part) else {
            return false;
        };
        if meta.file_type().is_symlink()
            || meta.uid() != 0
            || meta.mode() & 0o022 != 0
            || (first && !directory && !meta.is_file())
            || (!first && !meta.is_dir())
        {
            return false;
        }
        first = false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "e649bdfaf8453091931654bb81d777b4a5aa76cd";

    #[test]
    fn exact_command_accepts_only_one_lowercase_sha() {
        let command = format!("{HELPER} --head {SHA}");
        assert_eq!(parse_head(&command, HELPER), Some(SHA));
        for bad in [
            format!("{HELPER} --head {}", SHA.to_ascii_uppercase()),
            format!("{HELPER} --head {SHA} extra"),
            format!("{HELPER} --head {SHA}\n"),
            format!("{HELPER} --head {SHA}; true"),
            format!("{HELPER} --head {SHA} --proxy http://127.0.0.1"),
        ] {
            assert!(parse_head(&bad, HELPER).is_none(), "{bad}");
        }
    }

    #[test]
    fn proxy_requires_credential_free_loopback_http() {
        assert!(credential_free_loopback_http("http://127.0.0.1:1234"));
        for bad in [
            "https://127.0.0.1:1234",
            "http://user:pass@127.0.0.1:1234",
            "http://localhost:1234",
            "http://127.0.0.2:1234",
            "http://127.0.0.1:1234/path",
            "http://127.0.0.1",
        ] {
            assert!(!credential_free_loopback_http(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn unrelated_clients_keep_their_existing_launch() {
        let result = prepare(
            "git status",
            false,
            false,
            false,
            SandboxType::None,
            None,
            "local",
            &HashMap::new(),
        )
        .await
        .expect("ordinary command is not a managed launch");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn helper_requires_this_attempts_human_review_and_seatbelt() {
        let command = format!("{HELPER} --head {SHA}");
        for (approved, sandbox) in [
            (false, SandboxType::MacosSeatbelt),
            (true, SandboxType::None),
        ] {
            let result = prepare(
                &command,
                false,
                false,
                approved,
                sandbox,
                None,
                "local",
                &HashMap::new(),
            )
            .await;
            assert!(result.is_err());
        }
    }

    #[test]
    fn user_owned_issuer_cannot_be_accepted() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let image = temp.path().join("codex");
        fs::write(&image, b"placeholder").expect("temporary image");
        assert!(!root_sealed(&image, false));
    }
}
