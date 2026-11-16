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
        // Clients must not attempt unsupported discovery, filesystem streams,
        // snapshots or proxy setup on the process-only account service.
        info.capabilities = Default::default();
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
        assert_eq!(account.capabilities, Default::default());
        assert_eq!(account.executor_version, ordinary.executor_version);
        assert_eq!(account.provider_id, ordinary.provider_id);
    }
}
