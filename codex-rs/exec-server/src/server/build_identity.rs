//! Cache the running executor's build identity before accepting connections.

use std::sync::LazyLock;

use codex_build_info::BuildInfo;
use codex_build_info::build_id;

use crate::protocol::EnvironmentInfo;

pub(super) static PROVIDER_ID: LazyLock<Option<String>> = LazyLock::new(|| {
    let info = BuildInfo::get();
    info.target()
        .and_then(|target| build_id(info.build_commit(), target))
});

pub(super) fn local_environment_info(account_only: bool) -> EnvironmentInfo {
    let mut info = EnvironmentInfo {
        provider_id: PROVIDER_ID.clone(),
        ..super::release_version::local_environment_info()
    };
    if account_only {
        // Only process-local proxy enforcement crosses the account boundary.
        // Credentials, discovery, filesystem streams and snapshots remain unavailable.
        info.capabilities = info.capabilities.process_only();
    }
    info
}

#[cfg(test)]
mod tests {
    #[test]
    fn execution_account_advertises_no_unsupported_optional_capabilities() {
        let ordinary = super::local_environment_info(false);
        let account = super::local_environment_info(true);
        assert!(ordinary.capabilities.environment_config_read);
        assert_eq!(account.capabilities, ordinary.capabilities.process_only());
        assert!(account.capabilities.network_proxy().launch);
        assert!(!account.capabilities.environment_config_read);
        assert!(!account.capabilities.shell_snapshot_v2);
        assert_eq!(
            account.capabilities.network_proxy().credential_masking,
            codex_network_proxy::RemoteCredentialMasking::Disabled
        );
        assert_eq!(account.executor_version, ordinary.executor_version);
        assert_eq!(account.provider_id, ordinary.provider_id);
    }
}
