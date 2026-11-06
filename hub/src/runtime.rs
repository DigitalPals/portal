//! Ephemeral runtime ownership. Durable metadata is never authority to signal a PID.
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn require_bootstrap() -> bool {
    // Fail closed on misspelled nonempty values as well as true/1.
    std::env::var("PORTAL_HUB_REQUIRE_BOOTSTRAP")
        .is_ok_and(|v| !matches!(v.to_ascii_lowercase().as_str(), "" | "0" | "false"))
}

pub fn run_dir(state_dir: &Path) -> PathBuf {
    std::env::var_os("PORTAL_HUB_RUN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| state_dir.join("run"))
}

pub fn private_dir(path: &Path) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .context("failed to create private Hub directory")?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
        bail!("Hub directory must be a real directory owned by the service user");
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn ensure(state_dir: &Path) -> Result<PathBuf> {
    let dir = run_dir(state_dir);
    if !dir.is_absolute() || dir == state_dir || state_dir.starts_with(&dir) {
        bail!("PORTAL_HUB_RUN_DIR must be a dedicated absolute directory");
    }
    if dir.as_os_str().as_bytes().len() + "/sockets/".len() + 36 >= 108 {
        bail!("runtime socket paths are too long; choose a shorter PORTAL_HUB_RUN_DIR");
    }
    private_dir(&dir)?;
    for child in ["sockets", "ctl", "web-identities"] {
        private_dir(&dir.join(child))?;
    }
    Ok(dir)
}

fn private_file(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?)
}

/// Held by the web process, and automatically released on crash or termination.
pub fn service_lock(state_dir: &Path) -> Result<File> {
    let file = private_file(&ensure(state_dir)?.join("web.lock"))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        bail!("another Portal Hub web process owns this runtime directory");
    }
    Ok(file)
}

#[derive(Serialize, Deserialize)]
struct Generation {
    id: String,
    namespace: String,
}

fn namespace_identity() -> Result<String> {
    // Boot ID covers host reboot; PID namespace covers container replacement
    // even when an operator accidentally persists the run directory.
    Ok(format!(
        "{}:{}:{}",
        fs::read_to_string("/proc/sys/kernel/random/boot_id")?.trim(),
        fs::read_link("/proc/self/ns/pid")?.display(),
        process_start_time(&fs::read_to_string("/proc/1/stat")?)?,
    ))
}

pub fn generation(state_dir: &Path) -> Result<String> {
    let dir = ensure(state_dir)?;
    let lock = private_file(&dir.join("generation.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let path = dir.join("generation.json");
    let namespace = namespace_identity()?;
    if let Ok(mut file) = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
    {
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        if let Ok(existing) = serde_json::from_str::<Generation>(&contents)
            && existing.namespace == namespace
        {
            return Ok(existing.id);
        }
    }
    let generation = Generation {
        id: Uuid::new_v4().to_string(),
        namespace,
    };
    let temp = dir.join(format!(".generation-{}", Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    file.write_all(&serde_json::to_vec(&generation)?)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    Ok(generation.id)
}

pub fn live_socket(path: &Path) -> bool {
    match connect_socket(path) {
        Ok(_) => true,
        // A full accept queue is still a live session. Do not block maintenance
        // diagnostics, or declare a busy server dead and discard its socket.
        Err(error) => error.kind() == std::io::ErrorKind::WouldBlock,
    }
}

fn connect_socket(path: &Path) -> std::io::Result<UnixStream> {
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let bytes = path.as_os_str().as_bytes();
    if bytes.len() >= addr.sun_path.len() || bytes.contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid runtime socket path",
        ));
    }
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (target, byte) in addr.sun_path.iter_mut().zip(bytes) {
        *target = *byte as libc::c_char;
    }
    let fd = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    let len =
        (std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1) as libc::socklen_t;
    if unsafe {
        libc::connect(
            stream.as_raw_fd(),
            (&addr as *const libc::sockaddr_un).cast(),
            len,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(stream)
}

pub fn session_live(state_dir: &Path, id: Uuid, recorded_generation: Option<&str>) -> Result<bool> {
    let generation = generation(state_dir)?;
    Ok(recorded_generation == Some(generation.as_str())
        && live_socket(&run_dir(state_dir).join("sockets").join(id.to_string())))
}

/// Only signal the dtach server authenticated by a live private socket, through
/// a pidfd. Never use stored process/group IDs (they may identify another process).
pub fn terminate_socket(path: &Path) -> Result<bool> {
    let stream = match connect_socket(path) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
    };
    let mut peer: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut peer as *mut libc::ucred).cast(),
            &mut len,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    if peer.uid != unsafe { libc::geteuid() } {
        bail!("session socket is not owned by the service user");
    }
    // dtach binds before daemonizing, so SO_PEERCRED.pid can be its exited
    // parent. Resolve the actual listener owner through /proc, not saved PIDs.
    let socket_name = path.to_str().context("session socket path must be UTF-8")?;
    let inodes: Vec<String> = fs::read_to_string("/proc/net/unix")?
        .lines()
        .filter_map(|line| {
            let prefix = line.strip_suffix(socket_name)?;
            if !prefix.ends_with(char::is_whitespace) {
                return None;
            }
            let fields: Vec<_> = prefix.split_whitespace().collect();
            (fields.len() == 7 && fields[3] == "00010000")
                .then(|| format!("socket:[{}]", fields[6]))
        })
        .collect();
    let mut owner = None;
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
        else {
            continue;
        };
        if pid <= 1 {
            continue;
        }
        let proc_dir = entry.path();
        let Ok(executable) = fs::read_link(proc_dir.join("exe")) else {
            continue;
        };
        if executable.file_name().and_then(|s| s.to_str()) != Some("dtach") {
            continue;
        }
        let Ok(argv) = fs::read(proc_dir.join("cmdline")) else {
            continue;
        };
        if !argv
            .split(|b| *b == 0)
            .any(|arg| arg == path.as_os_str().as_bytes())
        {
            continue;
        }
        let Ok(before) = fs::read_to_string(proc_dir.join("stat")) else {
            continue;
        };
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                continue;
            }
            return Err(error).context("safe session termination requires Linux pidfd support");
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        let Ok(after) = fs::read_to_string(proc_dir.join("stat")) else {
            continue;
        };
        if process_start_time(&before)? != process_start_time(&after)? {
            continue;
        }
        let Ok(fds) = fs::read_dir(proc_dir.join("fd")) else {
            continue;
        };
        let owns_listener = fds
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| fs::read_link(entry.path()).ok())
            .any(|link| {
                inodes
                    .iter()
                    .any(|inode| link.as_os_str() == std::ffi::OsStr::new(inode))
            });
        if !owns_listener {
            continue;
        }
        if owner.is_some() {
            bail!("session socket has ambiguous process ownership");
        }
        owner = Some(fd);
    }
    let Some(fd) = owner else {
        if !live_socket(path) {
            return Ok(false);
        }
        bail!("could not verify the dtach session socket owner");
    };
    if unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            libc::SIGTERM,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    } != 0
    {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(false);
        }
        return Err(error.into());
    }
    Ok(true)
}

fn process_start_time(stat: &str) -> Result<&str> {
    // comm may contain spaces or parentheses. starttime is field 22.
    stat.rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().nth(19))
        .context("invalid process identity")
}

pub fn check_process_signaling() -> Result<()> {
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, std::process::id(), 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    // Signal 0 only checks permission/existence; it does not deliver a signal.
    if unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            0,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

pub fn clear_identities(state_dir: &Path) -> Result<()> {
    let current = ensure(state_dir)?.join("web-identities");
    // Legacy versions wrote keys into durable state. Clean these on migration too.
    for dir in [current, state_dir.join("web-identities")] {
        if !dir.exists() {
            continue;
        }
        private_dir(&dir)?;
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "key") {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::path::PathBuf::from("/tmp").join(format!("ph-{}", Uuid::new_v4())))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn generation_changes_after_runtime_replacement_or_namespace_change() {
        let fixture = Fixture::new();
        let first = generation(&fixture.0).unwrap();
        assert_eq!(generation(&fixture.0).unwrap(), first);
        fs::remove_dir_all(run_dir(&fixture.0)).unwrap();
        let second = generation(&fixture.0).unwrap();
        assert_ne!(first, second);
        let manifest = run_dir(&fixture.0).join("generation.json");
        fs::write(
            manifest,
            serde_json::to_vec(&Generation {
                id: second.clone(),
                namespace: "previous-container".into(),
            })
            .unwrap(),
        )
        .unwrap();
        assert_ne!(generation(&fixture.0).unwrap(), second);
    }

    #[test]
    fn socket_liveness_and_generation_are_both_required() {
        let fixture = Fixture::new();
        let generation = generation(&fixture.0).unwrap();
        let id = Uuid::new_v4();
        let socket = run_dir(&fixture.0).join("sockets").join(id.to_string());
        fs::write(&socket, "stale file").unwrap();
        assert!(!session_live(&fixture.0, id, Some(&generation)).unwrap());
        fs::remove_file(&socket).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert!(session_live(&fixture.0, id, Some(&generation)).unwrap());
        assert!(!session_live(&fixture.0, id, Some("previous-container")).unwrap());
        // A connectable socket owned by this test process must never authorize a signal.
        assert!(terminate_socket(&socket).is_err());
        drop(listener);
        assert!(!session_live(&fixture.0, id, Some(&generation)).unwrap());
    }

    #[test]
    fn full_socket_queue_is_active_without_blocking_diagnostics() {
        let fixture = Fixture::new();
        let socket = ensure(&fixture.0).unwrap().join("sockets/busy");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 1) }, 0);
        let mut queued = Vec::new();
        let mut full = false;
        for _ in 0..8 {
            match connect_socket(&socket) {
                Ok(connection) => queued.push(connection),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    full = true;
                    break;
                }
                Err(error) => panic!("unexpected socket error: {error}"),
            }
        }
        assert!(full);
        assert!(live_socket(&socket));
        assert!(terminate_socket(&socket).is_err());
    }

    #[test]
    fn private_runtime_rejects_symlinks_and_excludes_other_users() {
        let fixture = Fixture::new();
        let directory = ensure(&fixture.0).unwrap();
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let identities = directory.join("web-identities");
        fs::remove_dir(&identities).unwrap();
        std::os::unix::fs::symlink(&fixture.0, &identities).unwrap();
        assert!(ensure(&fixture.0).is_err());
    }

    #[test]
    fn web_service_lock_is_exclusive_and_released_on_drop() {
        let fixture = Fixture::new();
        let lock = service_lock(&fixture.0).unwrap();
        assert!(service_lock(&fixture.0).is_err());
        drop(lock);
        assert!(service_lock(&fixture.0).is_ok());
    }

    #[test]
    fn real_dtach_peer_termination_closes_the_detached_session() {
        if !crate::command_exists("dtach").unwrap_or(false) {
            eprintln!("skipping real dtach termination: enter the project development shell");
            return;
        }
        let fixture = Fixture::new();
        let socket = ensure(&fixture.0)
            .unwrap()
            .join("sockets")
            .join("termination");
        let status = std::process::Command::new("dtach")
            .arg("-n")
            .arg(&socket)
            .args(["sleep", "30"])
            .status()
            .unwrap();
        assert!(status.success());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !live_socket(&socket) {
            assert!(std::time::Instant::now() < deadline, "dtach did not start");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(terminate_socket(&socket).unwrap());
        while live_socket(&socket) {
            assert!(
                std::time::Instant::now() < deadline,
                "dtach did not terminate"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
