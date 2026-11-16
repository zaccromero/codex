//! Explicit host provisioning for a dedicated macOS execution account.
//! Account switching precedes the generated sandbox; workload code never does.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use codex_protocol::sandbox::SandboxOverride;
use serde::Deserialize;

#[cfg(target_os = "macos")]
const MAX_POLICY_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionAccountPolicy {
    schema_version: u32,
    username: String,
    uid: u32,
    gid: u32,
    home: PathBuf,
    root: PathBuf,
    path: String,
}

fn refused(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn plain_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path.to_str().is_some_and(|p| {
            !p.contains(['\0', '\n', '\r'])
                && p.split('/').skip(1).all(|c| !matches!(c, "" | "." | ".."))
        })
}

impl ExecutionAccountPolicy {
    fn validate(&self) -> io::Result<()> {
        if self.schema_version != 1
            || !(500..=i32::MAX as u32).contains(&self.uid)
            || self.gid > i32::MAX as u32
            || matches!(self.gid, 0 | 80)
            || self.username.is_empty()
            || self.username.len() > 32
            || !self.username.as_bytes()[0].is_ascii_lowercase()
            || !self
                .username
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
            || !plain_absolute(&self.home)
            || self.home.components().count() < 3
            || !plain_absolute(&self.root)
            || self.root == self.home
            || !self.root.starts_with(&self.home)
            || self.path.is_empty()
            || !self.path.split(':').all(|p| plain_absolute(Path::new(p)))
        {
            return Err(refused("invalid dedicated execution account policy"));
        }
        Ok(())
    }

    pub(crate) fn admit_cwd(&self, cwd: &Path) -> io::Result<()> {
        if !cwd.starts_with(&self.root) {
            return Err(refused(
                "execution account cwd escaped its provisioned root",
            ));
        }
        if !plain_absolute(cwd) {
            return Err(refused(
                "execution account cwd must be an absolute normal path",
            ));
        }
        let root = self.root.canonicalize()?;
        let actual = cwd.canonicalize()?;
        if root != self.root || !actual.starts_with(&root) {
            return Err(refused("execution account cwd has an escaping symlink"));
        }
        Ok(())
    }

    pub(crate) fn admit(
        &self,
        sandbox_override: SandboxOverride,
        has_denied_reads: bool,
        shell_snapshot: bool,
        custom_arg0: bool,
        managed_network: bool,
    ) -> io::Result<()> {
        self.validate()?;
        if sandbox_override != SandboxOverride::EscalatedSandboxWithRestrictions
            || !has_denied_reads
        {
            return Err(refused(
                "execution account requires explicit approved escalation with retained denied reads",
            ));
        }
        if shell_snapshot || custom_arg0 || managed_network {
            return Err(refused(
                "execution account does not support shell snapshots, custom argv0 or managed network proxies",
            ));
        }
        self.verify_identity()?;
        self.require_current_account()
    }

    // Owner environment and request environment values are deliberately not copied.
    pub(crate) fn clean_environment(&self) -> HashMap<String, String> {
        HashMap::from([
            ("HOME".into(), self.home.to_string_lossy().into_owned()),
            ("USER".into(), self.username.clone()),
            ("LOGNAME".into(), self.username.clone()),
            ("PATH".into(), self.path.clone()),
        ])
    }

    /// Detect the reserved mode before dotenv, helper aliases, threads or config.
    pub fn startup_policy_path(args: &[String]) -> io::Result<Option<PathBuf>> {
        if !args.iter().any(|arg| {
            arg == "--execution-account-policy" || arg.starts_with("--execution-account-policy=")
        }) {
            return Ok(None);
        }
        Self::validate_startup_args(args)?;
        let index = args
            .iter()
            .position(|arg| arg == "--execution-account-policy")
            .ok_or_else(|| refused("missing execution account policy"))?;
        Ok(Some(PathBuf::from(&args[index + 1])))
    }

    #[cfg(target_os = "macos")]
    fn validate_environment(&self, actual: &HashMap<String, String>) -> io::Result<()> {
        // CoreFoundation may synthesize this process-local preference encoding
        // marker after an otherwise empty `env -i` launch. It carries no owner
        // environment or credential data, so ignore only this macOS-owned key
        // while continuing to reject every other inherited variable.
        let mut actual = actual.clone();
        actual.remove("__CF_USER_TEXT_ENCODING");
        if actual != self.clean_environment() {
            return Err(refused(
                "account executor requires exactly its provisioned clean environment",
            ));
        }
        Ok(())
    }

    /// Loads only an administrator-provisioned policy. Request data cannot set it.
    pub fn from_file(path: &Path) -> io::Result<Self> {
        #[cfg(target_os = "macos")]
        {
            load_policy(path)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = path;
            Err(refused("dedicated execution accounts require macOS"))
        }
    }

    /// Validates the deliberately small bootstrap CLI before reading any config
    /// or credential input. Flags for remote auth/listeners/config overrides
    /// must never cross the account boundary.
    pub fn validate_startup_args(args: &[String]) -> io::Result<()> {
        let args = args
            .strip_prefix(&["exec-server".to_string()])
            .unwrap_or(args);
        let mut policy = false;
        let mut stdio = false;
        for pair in args.chunks(2) {
            match pair {
                [flag, value]
                    if flag == "--execution-account-policy"
                        && !policy
                        && plain_absolute(Path::new(value)) =>
                {
                    policy = true
                }
                [flag, value]
                    if flag == "--listen"
                        && !stdio
                        && matches!(value.as_str(), "stdio" | "stdio://") =>
                {
                    stdio = true
                }
                _ => {
                    return Err(refused(
                        "account executor accepts only --listen stdio and --execution-account-policy PATH",
                    ));
                }
            }
        }
        if policy && stdio {
            Ok(())
        } else {
            Err(refused(
                "account executor requires explicit stdio transport and policy",
            ))
        }
    }

    /// Re-execs the whole server as the dedicated account. Its existing process
    /// manager retains same-UID ownership of every job and descendant. There is
    /// no per-workload sudo wrapper and no privileged command interpreter.
    pub fn enter(&self, executable: &Path, policy_path: &Path) -> io::Result<()> {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (executable, policy_path);
            Err(refused("dedicated execution accounts require macOS"))
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::process::CommandExt;
            self.verify_identity()?;
            verify_trusted_file(executable)?;
            // SAFETY: geteuid has no preconditions.
            if unsafe { libc::geteuid() } == self.uid {
                self.require_current_account()?;
                self.validate_environment(&std::env::vars().collect())?;
                self.admit_cwd(&self.root)?;
                std::env::set_current_dir(&self.root)?;
                return Ok(());
            }
            let mut command = self.bootstrap_command(executable, policy_path)?;
            Err(command.exec())
        }
    }

    #[cfg(target_os = "macos")]
    fn bootstrap_command(
        &self,
        executable: &Path,
        policy_path: &Path,
    ) -> io::Result<std::process::Command> {
        self.validate()?;
        if !plain_absolute(executable) || !plain_absolute(policy_path) {
            return Err(refused(
                "account bootstrap paths must be absolute normal paths",
            ));
        }
        let mut command = std::process::Command::new("/usr/bin/sudo");
        command
            .args([
                "-n",
                "-H",
                "-u",
                &format!("#{}", self.uid),
                "-g",
                &format!("#{}", self.gid),
                "--",
            ])
            .args(["/usr/bin/env", "-i"]);
        // sudo may add variables of its own; clear again after changing UID.
        for key in ["HOME", "USER", "LOGNAME", "PATH"] {
            command.arg(format!("{key}={}", self.clean_environment()[key]));
        }
        command.arg(executable);
        match executable.file_name().and_then(|v| v.to_str()) {
            Some("codex") => {
                command.arg("exec-server");
            }
            Some("codex-exec-server") => {}
            _ => {
                return Err(refused(
                    "account bootstrap requires a packaged Codex executor",
                ));
            }
        }
        command
            .args(["--listen", "stdio", "--execution-account-policy"])
            .arg(policy_path)
            .current_dir("/")
            .env_clear()
            .envs(self.clean_environment());
        Ok(command)
    }
}

#[cfg(target_os = "macos")]
fn verify_trusted_file(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::OpenOptionsExt;
    if !plain_absolute(path) {
        return Err(refused(
            "account provisioning path must be absolute and normal",
        ));
    }
    for ancestor in path.ancestors() {
        let m = std::fs::symlink_metadata(ancestor)?;
        if m.file_type().is_symlink() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err(refused(
                "account provisioning file and parents must be root-owned, non-writable and not symlinks",
            ));
        }
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let m = file.metadata()?;
    if !m.is_file() || m.uid() != 0 || m.mode() & 0o022 != 0 {
        return Err(refused("invalid account provisioning file"));
    }
    Ok(file)
}

#[cfg(target_os = "macos")]
fn load_policy(path: &Path) -> io::Result<ExecutionAccountPolicy> {
    use std::io::Read;
    let file = verify_trusted_file(path)?;
    if file.metadata()?.len() > MAX_POLICY_BYTES {
        return Err(refused("account policy is too large"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_POLICY_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_POLICY_BYTES {
        return Err(refused("account policy is too large"));
    }
    let policy: ExecutionAccountPolicy =
        serde_json::from_slice(&bytes).map_err(|_| refused("invalid account policy JSON"))?;
    policy.validate()?;
    policy.verify_identity()?;
    Ok(policy)
}

#[cfg(target_os = "macos")]
impl ExecutionAccountPolicy {
    fn require_current_account(&self) -> io::Result<()> {
        // SAFETY: these identity queries have no preconditions.
        let ids = unsafe {
            (
                libc::getuid(),
                libc::geteuid(),
                libc::getgid(),
                libc::getegid(),
            )
        };
        if ids != (self.uid, self.uid, self.gid, self.gid) {
            return Err(refused("executor has not entered its dedicated account"));
        }
        Ok(())
    }
    fn verify_identity(&self) -> io::Result<()> {
        use std::ffi::CStr;
        use std::ffi::CString;
        let name =
            CString::new(self.username.as_str()).map_err(|_| refused("invalid account name"))?;
        let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut buffer = vec![0_u8; 64 * 1024];
        let mut found = std::ptr::null_mut();
        // SAFETY: all buffers remain live for the call and while reading fields.
        let status = unsafe {
            libc::getpwnam_r(
                name.as_ptr(),
                entry.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut found,
            )
        };
        if status != 0 || found.is_null() {
            return Err(refused("execution account lookup failed"));
        }
        // SAFETY: a successful lookup initialized entry and its string pointers.
        let entry = unsafe { entry.assume_init() };
        if entry.pw_dir.is_null() || entry.pw_name.is_null() {
            return Err(refused("execution account has no home"));
        }
        // SAFETY: pw_dir points into the live getpwnam_r buffer and is NUL-terminated.
        let home = unsafe { CStr::from_ptr(entry.pw_dir) }
            .to_str()
            .map_err(|_| refused("invalid account home"))?;
        // SAFETY: pw_name also points into the live initialized lookup buffer.
        let actual_name = unsafe { CStr::from_ptr(entry.pw_name) }.to_bytes();
        let mut groups = vec![0; 256];
        let mut count = groups.len() as libc::c_int;
        // SAFETY: the output array has count writable entries; oversized membership fails closed.
        let status = unsafe {
            libc::getgrouplist(
                name.as_ptr(),
                entry.pw_gid as libc::c_int,
                groups.as_mut_ptr(),
                &mut count,
            )
        };
        if actual_name != self.username.as_bytes()
            || entry.pw_uid != self.uid
            || entry.pw_gid != self.gid
            || Path::new(home) != self.home
            || status < 0
            || count < 0
            || count as usize > groups.len()
            || groups[..count as usize]
                .iter()
                .any(|gid| matches!(*gid, 0 | 80))
        {
            return Err(refused(
                "execution account identity changed or is privileged",
            ));
        }
        // A process launched directly under this UID must not retain groups
        // from a more privileged parent. sudo's normal initgroups path matches.
        if unsafe { libc::geteuid() } == self.uid {
            // SAFETY: zero queries the count without dereferencing the pointer.
            let size = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
            if !(0..=256).contains(&size) {
                return Err(refused("cannot validate execution account groups"));
            }
            let mut actual = vec![0; size as usize];
            // SAFETY: the vector has exactly size writable entries.
            let length = unsafe { libc::getgroups(size, actual.as_mut_ptr()) };
            if length != size {
                return Err(refused("execution account groups changed"));
            }
            if actual.iter().any(|group| {
                !groups[..count as usize]
                    .iter()
                    .any(|expected| *expected as u32 == *group)
            }) {
                return Err(refused(
                    "execution account retained foreign supplementary groups",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
impl ExecutionAccountPolicy {
    fn require_current_account(&self) -> io::Result<()> {
        Err(refused("dedicated execution accounts require macOS"))
    }
    fn verify_identity(&self) -> io::Result<()> {
        Err(refused("dedicated execution accounts require macOS"))
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "execution_account_tests.rs"]
mod tests;
