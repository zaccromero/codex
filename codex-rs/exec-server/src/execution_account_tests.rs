use super::*;

fn policy() -> ExecutionAccountPolicy {
    ExecutionAccountPolicy {
        schema_version: 1,
        username: "ci-user".into(),
        uid: 65001,
        gid: 20,
        home: "/Users/ci-user".into(),
        root: "/Users/ci-user/ci".into(),
        path: "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin".into(),
    }
}

#[test]
fn provisioning_rejects_privileged_or_ambiguous_targets() {
    let original = policy();
    original.validate().unwrap();
    let invalid = [
        ExecutionAccountPolicy {
            uid: 0,
            ..original.clone()
        },
        ExecutionAccountPolicy {
            uid: u32::MAX,
            ..original.clone()
        },
        ExecutionAccountPolicy {
            gid: u32::MAX,
            ..original.clone()
        },
        ExecutionAccountPolicy {
            uid: 499,
            ..original.clone()
        },
        ExecutionAccountPolicy {
            gid: 0,
            ..original.clone()
        },
        ExecutionAccountPolicy {
            gid: 80,
            ..original.clone()
        },
        ExecutionAccountPolicy {
            username: "-root".into(),
            ..original.clone()
        },
        ExecutionAccountPolicy {
            username: "ci;id".into(),
            ..original.clone()
        },
        ExecutionAccountPolicy {
            home: "/".into(),
            ..original.clone()
        },
        ExecutionAccountPolicy {
            root: original.home.clone(),
            ..original.clone()
        },
        ExecutionAccountPolicy {
            root: "/Users/ci-user-else/ci".into(),
            ..original.clone()
        },
        ExecutionAccountPolicy {
            root: "/Users/ci-user/ci/../outside".into(),
            ..original.clone()
        },
        ExecutionAccountPolicy {
            path: "/bin:relative".into(),
            ..original.clone()
        },
        ExecutionAccountPolicy {
            path: "/bin:".into(),
            ..original.clone()
        },
        ExecutionAccountPolicy {
            schema_version: 2,
            ..original.clone()
        },
    ];
    for candidate in invalid {
        assert!(candidate.validate().is_err());
    }
}

#[test]
fn policy_json_rejects_unknown_fields_and_types() {
    for json in [
        r#"{"schema_version":1,"username":"ci-user","uid":65001,"gid":20,"home":"/Users/ci-user","root":"/Users/ci-user/ci","path":"/bin","bypass_sandbox":true}"#,
        r#"{"schema_version":1,"username":"ci-user","uid":"0","gid":20,"home":"/Users/ci-user","root":"/Users/ci-user/ci","path":"/bin"}"#,
    ] {
        assert!(serde_json::from_str::<ExecutionAccountPolicy>(json).is_err());
    }
}

#[test]
fn startup_accepts_only_explicit_private_stdio_and_policy() {
    let valid = [
        "--listen",
        "stdio",
        "--execution-account-policy",
        "/Library/Codex/ci.json",
    ];
    let args = valid.map(str::to_owned).to_vec();
    ExecutionAccountPolicy::validate_startup_args(&args).unwrap();
    let mut full_cli = vec!["exec-server".to_owned()];
    full_cli.extend(args.clone());
    ExecutionAccountPolicy::validate_startup_args(&full_cli).unwrap();
    for extra in [
        vec!["--remote", "https://example.invalid"],
        vec!["--chatgpt-token-stdin"],
        vec!["-c", "approval_policy='never'"],
        vec!["--listen", "ws://127.0.0.1:1234"],
        vec!["--execution-account-policy", "/other.json"],
        vec!["--", "/bin/sh"],
    ] {
        let mut bad = args.clone();
        bad.extend(extra.into_iter().map(str::to_owned));
        assert!(ExecutionAccountPolicy::validate_startup_args(&bad).is_err());
    }
    assert!(ExecutionAccountPolicy::validate_startup_args(&[]).is_err());
    assert!(ExecutionAccountPolicy::validate_startup_args(&args[..2]).is_err());
}

#[test]
fn bootstrap_starts_an_executor_not_a_workload_or_privileged_shell() {
    let p = policy();
    for name in ["codex", "codex-exec-server"] {
        let executable = format!("/Library/Codex/{name}");
        let command = p
            .bootstrap_command(Path::new(&executable), Path::new("/Library/Codex/ci.json"))
            .unwrap();
        assert_eq!(command.get_program(), "/usr/bin/sudo");
        let args: Vec<_> = command.get_args().map(|s| s.to_str().unwrap()).collect();
        assert_eq!(
            &args[..14],
            &[
                "-n",
                "-H",
                "-u",
                "#65001",
                "-g",
                "#20",
                "--",
                "/usr/bin/env",
                "-i",
                "HOME=/Users/ci-user",
                "USER=ci-user",
                "LOGNAME=ci-user",
                "PATH=/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
                executable.as_str()
            ]
        );
        let offset = if name == "codex" {
            assert_eq!(args[14], "exec-server");
            15
        } else {
            14
        };
        assert_eq!(
            &args[offset..],
            &[
                "--listen",
                "stdio",
                "--execution-account-policy",
                "/Library/Codex/ci.json"
            ]
        );
        assert_eq!(command.get_current_dir(), Some(Path::new("/")));
        let env: HashMap<_, _> = command
            .get_envs()
            .map(|(k, v)| (k.to_str().unwrap(), v.unwrap().to_str().unwrap()))
            .collect();
        assert_eq!(env.len(), 4);
        assert_eq!(env["HOME"], "/Users/ci-user");
        assert_eq!(env["USER"], "ci-user");
        assert!(!env.contains_key("GH_TOKEN"));
        assert!(!env.contains_key("CODEX_HOME"));
        assert!(!env.contains_key("BASH_ENV"));
    }
    for executable in ["/bin/sh", "codex", "/Library/Codex/../codex"] {
        assert!(
            p.bootstrap_command(Path::new(executable), Path::new("/Library/Codex/ci.json"))
                .is_err()
        );
    }
}

#[test]
fn command_environment_contains_only_account_identity_and_provisioned_path() {
    let env = policy().clean_environment();
    assert_eq!(env.len(), 4);
    assert_eq!(env["HOME"], "/Users/ci-user");
    assert_eq!(env["LOGNAME"], "ci-user");
    for key in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "DATABASE_URL",
        "DOPPLER_TOKEN",
        "NODE_OPTIONS",
        "BASH_ENV",
        "ENV",
        "SSH_AUTH_SOCK",
        "DYLD_INSERT_LIBRARIES",
    ] {
        assert!(!env.contains_key(key));
    }
}

#[test]
fn unsafe_request_modes_fail_before_account_lookup_or_launch() {
    let p = policy();
    for (approval, denies, snapshot, arg0, proxy) in [
        (SandboxOverride::NoOverride, true, false, false, false),
        (
            SandboxOverride::BypassSandboxFirstAttempt,
            true,
            false,
            false,
            false,
        ),
        (
            SandboxOverride::EscalatedSandboxWithRestrictions,
            false,
            false,
            false,
            false,
        ),
        (
            SandboxOverride::EscalatedSandboxWithRestrictions,
            true,
            true,
            false,
            false,
        ),
        (
            SandboxOverride::EscalatedSandboxWithRestrictions,
            true,
            false,
            true,
            false,
        ),
        (
            SandboxOverride::EscalatedSandboxWithRestrictions,
            true,
            false,
            false,
            true,
        ),
    ] {
        let error = p
            .admit(approval, denies, snapshot, arg0, proxy)
            .unwrap_err()
            .to_string();
        assert!(
            !error.contains("lookup"),
            "must fail before inspecting the account: {error}"
        );
    }
}

#[test]
fn malformed_and_foreign_working_directories_are_rejected() {
    let p = policy();
    for cwd in [
        "/Users/ci-user/other",
        "/Users/ci-user/ci-else",
        "/Users/ci-user/ci/../other",
        "/Users/ci-user/ci/./test",
        "relative",
        "/Users/ci-user/ci/test\nnext",
    ] {
        assert!(p.admit_cwd(Path::new(cwd)).is_err());
    }
}

#[cfg(unix)]
#[test]
fn existing_private_cwd_accepts_internal_links_but_rejects_escapes() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    let root = home.join("ci");
    let child = root.join("job");
    let outside = home.join("elsewhere");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::create_dir(&outside).unwrap();
    symlink(&outside, root.join("escape")).unwrap();
    symlink(&child, root.join("internal")).unwrap();
    let p = ExecutionAccountPolicy {
        home,
        root: root.clone(),
        ..policy()
    };
    p.admit_cwd(&child).unwrap();
    p.admit_cwd(&root.join("internal")).unwrap();
    assert!(p.admit_cwd(&root.join("escape")).is_err());
    assert!(p.admit_cwd(&root.join("missing")).is_err());
    let alias = root.join("alias-root");
    symlink(&child, &alias).unwrap();
    let p = ExecutionAccountPolicy { root: alias, ..p };
    assert!(p.admit_cwd(&p.root).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn startup_rejects_user_writable_provisioning_files_and_symlinks() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("policy.json");
    std::fs::write(&path, b"{}").unwrap();
    assert!(load_policy(&path).is_err());
    let alias = root.join("alias.json");
    symlink(&path, &alias).unwrap();
    assert!(load_policy(&alias).is_err());
    assert!(verify_trusted_file(&path).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn executor_refuses_running_as_the_wrong_uid() {
    let mut p = policy();
    // SAFETY: geteuid has no preconditions. Pick an intentionally different ID.
    p.uid = unsafe { libc::geteuid() } + 10_000;
    assert!(p.require_current_account().is_err());
}

#[test]
fn account_startup_is_detected_before_loading_environment() {
    for ordinary in [
        vec![],
        vec!["exec", "hello"],
        vec!["exec-server", "--listen", "stdio"],
    ] {
        let args = ordinary.into_iter().map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(
            ExecutionAccountPolicy::startup_policy_path(&args).unwrap(),
            None
        );
    }
    let valid = [
        "exec-server",
        "--listen",
        "stdio",
        "--execution-account-policy",
        "/Library/Codex/ci.json",
    ]
    .map(str::to_owned);
    assert_eq!(
        ExecutionAccountPolicy::startup_policy_path(&valid).unwrap(),
        Some("/Library/Codex/ci.json".into())
    );
    for invalid in [
        vec!["--execution-account-policy=/Library/Codex/ci.json"],
        vec!["--execution-account-policy"],
        vec![
            "exec",
            "--execution-account-policy",
            "/Library/Codex/ci.json",
        ],
    ] {
        let args = invalid.into_iter().map(str::to_owned).collect::<Vec<_>>();
        assert!(ExecutionAccountPolicy::startup_policy_path(&args).is_err());
    }
}

#[test]
fn direct_account_startup_rejects_environment_injection_and_wrong_home() {
    let p = policy();
    p.validate_environment(&p.clean_environment()).unwrap();
    let mut synthesized = p.clean_environment();
    synthesized.insert("__CF_USER_TEXT_ENCODING".into(), "0x1F6:0x0:0x0".into());
    p.validate_environment(&synthesized).unwrap();
    assert!(
        !p.clean_environment()
            .contains_key("__CF_USER_TEXT_ENCODING")
    );
    for key in [
        "HOME",
        "USER",
        "LOGNAME",
        "PATH",
        "CODEX_HOME",
        "GH_TOKEN",
        "BASH_ENV",
        "DYLD_INSERT_LIBRARIES",
        "XDG_CONFIG_HOME",
    ] {
        let mut env = p.clean_environment();
        env.insert(key.into(), "/untrusted".into());
        assert!(p.validate_environment(&env).is_err(), "accepted {key}");
    }
    assert!(p.validate_environment(&HashMap::new()).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn trusted_file_validation_accepts_system_owned_regular_file() {
    verify_trusted_file(Path::new("/usr/bin/true")).unwrap();
}
