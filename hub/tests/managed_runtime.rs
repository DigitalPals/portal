use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

struct Fixture {
    root: PathBuf,
    server: Option<Child>,
}

impl Fixture {
    fn new() -> Self {
        let root = std::path::PathBuf::from("/tmp").join(format!("ph-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self { root, server: None }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_portal-hub"));
        cmd.arg("--state-dir")
            .arg(self.root.join("data"))
            .env("PORTAL_HUB_RUN_DIR", self.root.join("run"))
            .env("PORTAL_HUB_REQUIRE_BOOTSTRAP", "true")
            .env("PORTAL_HUB_NETWORK_MODE", "sidecar")
            .env_remove("SSH_ORIGINAL_COMMAND")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn start_bootstrap(&self, username: &str, password: &str) -> Child {
        let mut child = self
            .command()
            .args(["bootstrap-owner", "--json"])
            .spawn()
            .unwrap();
        let input = json!({"username": username, "password": password}).to_string();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child
    }

    fn bootstrap(&self, username: &str, password: &str) -> Output {
        self.start_bootstrap(username, password)
            .wait_with_output()
            .unwrap()
    }

    fn start_web(&mut self) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        self.server = Some(
            self.command()
                .args(["web", "--bind", &format!("127.0.0.1:{port}")])
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return port;
            }
            assert!(
                self.server.as_mut().unwrap().try_wait().unwrap().is_none(),
                "web server exited at startup"
            );
            assert!(Instant::now() < deadline, "web startup timeout");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn stop_web(&mut self) {
        let mut server = self.server.take().unwrap();
        assert_eq!(unsafe { libc::kill(server.id() as i32, libc::SIGTERM) }, 0);
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(status) = server.try_wait().unwrap() {
                assert!(status.success(), "server did not shut down cleanly");
                return;
            }
            if Instant::now() >= deadline {
                let _ = server.kill();
                let _ = server.wait();
                panic!("graceful shutdown exceeded its bound");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(server) = &mut self.server {
            let _ = server.kill();
            let _ = server.wait();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn response(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn bootstrap_cli_is_local_idempotent_and_never_replaces_credentials() {
    let fixture = Fixture::new();
    assert_eq!(
        response(
            fixture
                .command()
                .args(["bootstrap-status", "--json"])
                .output()
                .unwrap()
        ),
        json!({"owner_exists":false})
    );
    assert!(
        !fixture.root.join("data").exists(),
        "status should not create state"
    );
    assert_eq!(
        response(fixture.bootstrap("owner", "unique-test-password")),
        json!({"owner_exists":true, "created":true})
    );
    assert_eq!(
        response(fixture.bootstrap("owner", "unique-test-password")),
        json!({"owner_exists":true, "created":false})
    );
    assert!(
        !fixture
            .bootstrap("someone-else", "unique-test-password")
            .status
            .success()
    );
    assert!(
        !fixture
            .bootstrap("owner", "some-wrong-password")
            .status
            .success()
    );
    let denied = fixture
        .command()
        .args(["serve", "--stdio"])
        .env("SSH_ORIGINAL_COMMAND", "portal-hub bootstrap-owner --json")
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("local service administrator"));
    let db = rusqlite::Connection::open(fixture.root.join("data/hub.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        fs::metadata(fixture.root.join("data"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn independently_started_bootstraps_elect_exactly_one_owner() {
    let fixture = Fixture::new();
    let attempts: Vec<_> = (0..6)
        .map(|i| fixture.start_bootstrap(&format!("owner-{i}"), "unique-test-password"))
        .collect();
    let results: Vec<_> = attempts
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|r| r.status.success()).count(), 1);
    let winner = results.iter().position(|r| r.status.success()).unwrap();
    assert_eq!(
        response(fixture.bootstrap(&format!("owner-{winner}"), "unique-test-password"))["created"],
        false
    );
}

#[test]
fn bootstrap_parser_does_not_echo_secrets_or_accept_oversized_input() {
    let fixture = Fixture::new();
    for input in [
        "{\"username\":\"owner\",\"password\":secret-that-must-not-leak}".to_string(),
        "X".repeat(8193),
    ] {
        let mut child = fixture
            .command()
            .args(["bootstrap-owner", "--json"])
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret-that-must-not-leak"));
    }
    assert!(!fixture.root.join("data/hub.db").exists());
}

#[test]
fn managed_http_cannot_claim_owner_and_bootstrap_is_visible_without_restart() {
    let mut fixture = Fixture::new();
    let port = fixture.start_web();
    let body = json!({"username":"public-owner", "password":"unique-test-password"}).to_string();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream, "POST /auth/register HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
    assert_eq!(
        response(fixture.bootstrap("local-owner", "unique-test-password"))["created"],
        true
    );
    assert_eq!(
        response(
            fixture
                .command()
                .args(["bootstrap-status", "--json"])
                .output()
                .unwrap()
        )["owner_exists"],
        true
    );
    fixture.stop_web();
    fixture.start_web();
    assert_eq!(
        response(fixture.bootstrap("local-owner", "unique-test-password"))["created"],
        false
    );
    fixture.stop_web();
}

#[test]
fn doctor_sidecar_checks_real_prerequisites_without_requiring_tailscale() {
    let fixture = Fixture::new();
    response(fixture.bootstrap("owner", "unique-test-password"));
    let bin = fixture.root.join("bin");
    fs::create_dir(&bin).unwrap();
    for name in ["sh", "ssh", "ssh-keygen", "ssh-keyscan", "script", "dtach"] {
        symlink("/bin/sh", bin.join(name)).unwrap();
    }
    let output = fixture
        .command()
        .args(["doctor", "--json"])
        .env("PATH", &bin)
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["network_mode"], "sidecar");
    assert_eq!(report["owner_exists"], true);
    let checks = report["checks"].as_array().unwrap();
    assert!(
        !checks
            .iter()
            .any(|check| check["name"] == "binary:tailscale")
    );
    assert!(
        checks
            .iter()
            .find(|check| check["name"] == "binary:dtach")
            .unwrap()["ok"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(output.status.success(), unsafe { libc::geteuid() } != 0);
    fs::remove_file(bin.join("ssh-keyscan")).unwrap();
    let output = fixture
        .command()
        .args(["doctor", "--json"])
        .env("PATH", &bin)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "binary:ssh-keyscan")
            .unwrap()["ok"],
        false
    );
}

#[test]
fn replaced_runtime_reconciles_metadata_and_preserves_durable_data() {
    let mut fixture = Fixture::new();
    response(fixture.bootstrap("owner", "unique-test-password"));
    fixture.start_web();
    fixture.stop_web();
    let session_id = uuid::Uuid::new_v4();
    let metadata = json!({"schema_version":2,"session_id":session_id,"session_name":"old", "target_host":"host", "target_port":22, "target_user":"user", "created_at":chrono::Utc::now(), "updated_at":chrono::Utc::now(), "ended_at":null, "process_id":std::process::id(), "process_group_id":unsafe {libc::getpgrp()}, "runtime_generation":"previous-container"});
    let metadata_path = fixture
        .root
        .join("data/sessions")
        .join(format!("{session_id}.json"));
    fs::write(&metadata_path, metadata.to_string()).unwrap();
    fs::write(
        fixture
            .root
            .join("data/logs")
            .join(format!("{session_id}.typescript")),
        "retained output",
    )
    .unwrap();
    fs::write(
        fixture.root.join("run/web-identities/old.key"),
        "ephemeral key",
    )
    .unwrap();
    fs::write(
        fixture
            .root
            .join("run/sockets")
            .join(session_id.to_string()),
        "stale socket",
    )
    .unwrap();
    fixture.start_web();
    let ended: Value = serde_json::from_slice(&fs::read(&metadata_path).unwrap()).unwrap();
    assert!(!ended["ended_at"].is_null());
    assert!(ended["process_id"].is_null());
    assert!(ended["process_group_id"].is_null());
    assert!(!fixture.root.join("run/web-identities/old.key").exists());
    assert!(!fixture.root.join("data/sockets").exists());
    assert_eq!(
        response(
            fixture
                .command()
                .args(["list", "--active"])
                .output()
                .unwrap()
        ),
        json!([])
    );
    assert_eq!(
        response(fixture.bootstrap("owner", "unique-test-password"))["created"],
        false
    );
    fixture.stop_web();
    assert_eq!(
        fs::read_to_string(
            fixture
                .root
                .join("data/logs")
                .join(format!("{session_id}.typescript"))
        )
        .unwrap(),
        "retained output"
    );
}

#[test]
fn graceful_web_shutdown_terminates_live_detached_sessions() {
    if !Command::new("sh")
        .args(["-c", "command -v dtach"])
        .stdout(Stdio::null())
        .status()
        .unwrap()
        .success()
    {
        eprintln!("skipping real dtach shutdown: enter the project development shell");
        return;
    }
    let mut fixture = Fixture::new();
    response(fixture.bootstrap("owner", "unique-test-password"));
    fixture.start_web();
    let session_id = uuid::Uuid::new_v4();
    let socket = fixture
        .root
        .join("run/sockets")
        .join(session_id.to_string());
    let generation: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("run/generation.json")).unwrap())
            .unwrap();
    let metadata = json!({"schema_version":2,"session_id":session_id,"session_name":"live", "target_host":"host", "target_port":22, "target_user":"user", "created_at":chrono::Utc::now(), "updated_at":chrono::Utc::now(), "ended_at":null, "runtime_generation":generation["id"]});
    let metadata_path = fixture
        .root
        .join("data/sessions")
        .join(format!("{session_id}.json"));
    fs::write(&metadata_path, metadata.to_string()).unwrap();
    assert!(
        Command::new("dtach")
            .arg("-n")
            .arg(&socket)
            .args(["sleep", "30"])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while std::os::unix::net::UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < deadline, "detached session did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        response(
            fixture
                .command()
                .args(["list", "--active"])
                .output()
                .unwrap()
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    fixture.stop_web();
    assert!(std::os::unix::net::UnixStream::connect(socket).is_err());
    let ended: Value = serde_json::from_slice(&fs::read(metadata_path).unwrap()).unwrap();
    assert!(!ended["ended_at"].is_null());
}
