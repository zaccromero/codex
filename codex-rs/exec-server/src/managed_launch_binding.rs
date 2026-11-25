//! One-use, kernel-peer-bound launch channel for owner-installed local helpers.
//!
//! This is a transport primitive, not an approval source. Only the in-process
//! core approval path may create it. Remote `ExecParams` and child environment
//! fields cannot request one.

use std::fmt;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tempfile::Builder;
use tempfile::TempDir;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixListener;

pub const MANAGED_LAUNCH_SOCKET_ENV: &str = "CODEX_MANAGED_LAUNCH_SOCKET";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ChildIdentity {
    start_seconds: u64,
    start_microseconds: u64,
    parent_pid: u32,
    uid: libc::uid_t,
}

#[derive(Clone, Serialize)]
pub struct ManagedLaunchClaims {
    pub helper: String,
    pub helper_sha256: String,
    pub argv: Vec<String>,
    pub https_proxy: String,
    pub destination: String,
    pub release_host: String,
    pub issuer_exe: String,
}

#[derive(Serialize)]
struct BoundResponse<'a> {
    version: u8,
    nonce: String,
    child_pid: u32,
    issuer_pid: u32,
    issuer_exe: &'a str,
    helper: &'a str,
    helper_sha256: &'a str,
    argv: &'a [String],
    https_proxy: &'a str,
    destination: &'a str,
    release_host: &'a str,
}

pub struct ManagedLaunchBinding {
    _socket_dir: TempDir,
    listener: Mutex<Option<UnixListener>>,
    socket_path: String,
    claims: ManagedLaunchClaims,
}

impl fmt::Debug for ManagedLaunchBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ManagedLaunchBinding(<redacted>)")
    }
}

impl ManagedLaunchBinding {
    pub fn new(claims: ManagedLaunchClaims) -> io::Result<Arc<Self>> {
        let socket_dir = Builder::new()
            .prefix("codex-managed-launch-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/private/tmp")?;
        let path = socket_dir.path().join("binding.sock");
        let std_listener = std::os::unix::net::UnixListener::bind(&path)?;
        std_listener.set_nonblocking(true)?;
        let listener = UnixListener::from_std(std_listener)?;
        Ok(Arc::new(Self {
            _socket_dir: socket_dir,
            listener: Mutex::new(Some(listener)),
            socket_path: path.to_string_lossy().into_owned(),
            claims,
        }))
    }

    pub fn socket_path(&self) -> &str {
        &self.socket_path
    }

    /// Start the one accepted handshake after the exact child has spawned.
    /// An invalid peer consumes the channel; the helper then fails closed.
    pub fn serve_spawned_child(self: &Arc<Self>, child_pid: u32) -> io::Result<()> {
        // The PID alone can be recycled. Capture the kernel start identity while
        // the process is still our child, then recheck it before releasing facts.
        let child_identity = read_child_identity(child_pid)?;
        let listener = self
            .listener
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::AlreadyExists, "launch already served"))?;
        let binding = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(err) = binding.serve(listener, child_pid, child_identity).await {
                tracing::warn!("managed launch handshake refused: {err}");
            }
        });
        Ok(())
    }

    async fn serve(
        &self,
        listener: UnixListener,
        child_pid: u32,
        child_identity: ChildIdentity,
    ) -> io::Result<()> {
        let (mut stream, _) = tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
            let mut check = tokio::time::interval(Duration::from_millis(100));
            loop {
                tokio::select! {
                    biased;
                    _ = check.tick() => verify_child_identity(child_pid, child_identity)?,
                    accepted = listener.accept() => return accepted,
                }
            }
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "launch handshake timed out"))??;
        if peer_pid(stream.as_raw_fd())? != child_pid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "launch peer is not the spawned child",
            ));
        }
        verify_child_identity(child_pid, child_identity)?;
        let mut nonce = [0_u8; 32];
        tokio::time::timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut nonce))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "launch nonce timed out"))??;
        verify_child_identity(child_pid, child_identity)?;
        let response = BoundResponse {
            version: 1,
            nonce: nonce
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(""),
            child_pid,
            issuer_pid: std::process::id(),
            issuer_exe: &self.claims.issuer_exe,
            helper: &self.claims.helper,
            helper_sha256: &self.claims.helper_sha256,
            argv: &self.claims.argv,
            https_proxy: &self.claims.https_proxy,
            destination: &self.claims.destination,
            release_host: &self.claims.release_host,
        };
        let mut data = serde_json::to_vec(&response).map_err(io::Error::other)?;
        data.push(b'\n');
        tokio::time::timeout(HANDSHAKE_TIMEOUT, stream.write_all(&data))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "launch response timed out"))??;
        stream.shutdown().await
    }
}

fn read_child_identity(pid: u32) -> io::Result<ChildIdentity> {
    let pid = libc::pid_t::try_from(pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "child PID out of range"))?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer has the exact size and alignment of PROC_PIDTBSDINFO.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            /*arg*/ 1,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if read <= 0 {
        return Err(io::Error::last_os_error());
    }
    if read != size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "incomplete child identity",
        ));
    }
    // SAFETY: proc_pidinfo initialized the entire structure on success.
    let info = unsafe { info.assume_init() };
    if info.pbi_pid != pid as u32
        || info.pbi_ppid != std::process::id()
        || info.pbi_uid != unsafe { libc::geteuid() }
        || info.pbi_status == libc::SZOMB
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "launch child changed or exited",
        ));
    }
    Ok(ChildIdentity {
        start_seconds: info.pbi_start_tvsec,
        start_microseconds: info.pbi_start_tvusec,
        parent_pid: info.pbi_ppid,
        uid: info.pbi_uid,
    })
}

fn verify_child_identity(pid: u32, expected: ChildIdentity) -> io::Result<()> {
    if read_child_identity(pid)? != expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "launch child identity changed",
        ));
    }
    Ok(())
}

fn peer_pid(fd: std::os::fd::RawFd) -> io::Result<u32> {
    // Darwin's LOCAL_PEERPID is a kernel credential of a connected AF_UNIX
    // stream. It is checked on accept, not accepted from a request body.
    let mut pid: libc::pid_t = 0;
    let mut length = std::mem::size_of_val(&pid) as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            fd,
            0, // SOL_LOCAL
            2, // LOCAL_PEERPID
            std::ptr::from_mut(&mut pid).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of_val(&pid) || pid <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid launch peer PID",
        ));
    }
    Ok(pid as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::process::Command;

    fn claims() -> ManagedLaunchClaims {
        ManagedLaunchClaims {
            helper: "/reviewed/installer".to_string(),
            helper_sha256: "a".repeat(64),
            argv: vec!["/reviewed/installer".to_string()],
            https_proxy: "http://127.0.0.1:12345".to_string(),
            destination: "https://github.com/obsidianmd/obsidian-releases".to_string(),
            release_host: "release-assets.githubusercontent.com".to_string(),
            issuer_exe: "/reviewed/bin/codex".to_string(),
        }
    }

    #[tokio::test]
    async fn spawned_child_receives_one_exact_response() -> anyhow::Result<()> {
        let binding = ManagedLaunchBinding::new(claims())?;
        let script = r#"
import json, socket, sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(sys.argv[1])
s.sendall(b'Z' * 32)
data = s.recv(4096)
while not data.endswith(b'\n'):
    more = s.recv(4096)
    if not more: break
    data += more
proof = json.loads(data)
assert proof['nonce'] == '5a' * 32
assert proof['child_pid'] > 0
assert proof['argv'] == ['/reviewed/installer']
assert proof['release_host'] == 'release-assets.githubusercontent.com'
print('verified')
"#;
        // The PID is required before the child connects. Spawn explicitly.
        let mut command = Command::new("/usr/bin/python3");
        command
            .arg("-B")
            .arg("-c")
            .arg(script)
            .arg(binding.socket_path())
            .stdout(std::process::Stdio::piped());
        let process = command.spawn()?;
        let pid = process.id().expect("spawned child PID");
        binding.serve_spawned_child(pid)?;
        let output = process.wait_with_output().await?;
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "verified");
        assert!(binding.serve_spawned_child(pid).is_err());
        Ok(())
    }

    #[tokio::test]
    async fn wrong_peer_pid_gets_no_binding() -> anyhow::Result<()> {
        let binding = ManagedLaunchBinding::new(claims())?;
        let mut child = Command::new("/bin/sleep").arg("5").spawn()?;
        binding.serve_spawned_child(child.id().expect("spawned child PID"))?;
        let mut stream = tokio::net::UnixStream::connect(binding.socket_path()).await?;
        let mut data = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut data)).await?;
        assert!(data.is_empty());
        child.start_kill()?;
        child.wait().await?;
        Ok(())
    }

    #[tokio::test]
    async fn exited_child_cannot_leave_reusable_binding() -> anyhow::Result<()> {
        let binding = ManagedLaunchBinding::new(claims())?;
        let mut child = Command::new("/bin/sleep").arg("5").spawn()?;
        let pid = child.id().expect("spawned child PID");
        binding.serve_spawned_child(pid)?;
        child.start_kill()?;
        child.wait().await?;
        if let Ok(mut stream) = tokio::net::UnixStream::connect(binding.socket_path()).await {
            let _ = stream.write_all(&[7_u8; 32]).await;
            let mut data = Vec::new();
            let _ =
                tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut data)).await?;
            assert!(data.is_empty());
        }
        Ok(())
    }
}
