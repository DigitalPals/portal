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

    fn owner_setup(&self, args: &[&str]) -> Output {
        self.command()
            .arg("owner-setup")
            .arg("--json")
            .args(args)
            .output()
            .unwrap()
    }

    fn status(&self) -> Value {
        response(
            self.command()
                .args(["bootstrap-status", "--json"])
                .output()
                .unwrap(),
        )
    }

    fn db(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.join("data/hub.db")).unwrap()
    }

    /// An owner who already finished passkey setup. The passkey content is
    /// opaque to these runtime tests; ceremonies are covered by unit tests.
    fn seed_owner(&self) {
        response(self.owner_setup(&["--username", "owner"]));
        let db = self.db();
        db.execute(
            "INSERT INTO users (id, username, created_at) VALUES ('owner-id', 'owner', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO passkeys (id, user_id, passkey, created_at) VALUES ('passkey-id', 'owner-id', '{}', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        db.execute("DELETE FROM owner_setup_tokens", []).unwrap();
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

fn token_hash(token: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn owner_setup_links_are_local_single_and_stored_only_as_hashes() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.status(),
        json!({"owner_exists":false, "owner_ready":false})
    );
    assert!(
        !fixture.root.join("data").exists(),
        "status should not create state"
    );
    // A new owner needs a name; other modes need an existing owner.
    assert!(!fixture.owner_setup(&[]).status.success());
    assert!(!fixture.owner_setup(&["--mode", "add"]).status.success());
    assert!(!fixture.owner_setup(&["--mode", "reset"]).status.success());
    assert!(
        !fixture
            .owner_setup(&["--username", "bad name!"])
            .status
            .success()
    );

    let first = response(fixture.owner_setup(&["--username", "owner"]));
    assert_eq!(first["username"], "owner");
    assert_eq!(first["mode"], "setup");
    assert!(
        first["setup_url"].is_null(),
        "no HTTPS address is known yet"
    );
    let token = first["token"].as_str().unwrap();
    assert_eq!(token.len(), 43);
    // The link only authorizes creating the owner; it does not create one.
    assert_eq!(
        fixture.status(),
        json!({"owner_exists":false, "owner_ready":false})
    );
    let second = response(
        fixture
            .command()
            .args(["owner-setup", "--json", "--username", "owner"])
            .env(
                "PORTAL_HUB_PUBLIC_URL",
                "https://portal-hub.example.ts.net/",
            )
            .output()
            .unwrap(),
    );
    let second_token = second["token"].as_str().unwrap();
    assert_eq!(
        second["setup_url"],
        format!("https://portal-hub.example.ts.net/setup#token={second_token}")
    );
    let stored: Vec<String> = fixture
        .db()
        .prepare("SELECT token_hash FROM owner_setup_tokens")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(stored, vec![token_hash(second_token)]);

    for denied in [
        fixture
            .command()
            .args(["serve", "--stdio"])
            .env(
                "SSH_ORIGINAL_COMMAND",
                "portal-hub owner-setup --json --username owner",
            )
            .output()
            .unwrap(),
        fixture
            .command()
            .args(["owner-setup", "--json", "--username", "owner"])
            .env("SSH_ORIGINAL_COMMAND", "anything")
            .output()
            .unwrap(),
    ] {
        assert!(!denied.status.success());
        assert!(String::from_utf8_lossy(&denied.stderr).contains("local service administrator"));
        assert!(denied.stdout.is_empty());
    }
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
fn concurrent_setup_links_leave_exactly_one_valid_link() {
    let fixture = Fixture::new();
    response(fixture.owner_setup(&["--username", "owner"]));
    let attempts: Vec<_> = (0..6)
        .map(|i| {
            fixture
                .command()
                .args(["owner-setup", "--json", "--username", &format!("owner-{i}")])
                .spawn()
                .unwrap()
        })
        .collect();
    let tokens: Vec<String> = attempts
        .into_iter()
        .map(|child| {
            response(child.wait_with_output().unwrap())["token"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    let stored: String = fixture
        .db()
        .query_row("SELECT token_hash FROM owner_setup_tokens", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        tokens
            .iter()
            .filter(|token| token_hash(token) == stored)
            .count(),
        1
    );
}

#[test]
fn web_never_creates_an_owner_and_sees_cli_changes_without_restart() {
    let mut fixture = Fixture::new();
    let port = fixture.start_web();
    let request = |method: &str, path: &str, body: &str| {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        reply
    };
    let password =
        json!({"username":"public-owner", "password":"unique-test-password"}).to_string();
    for path in ["/auth/register", "/auth/login"] {
        let reply = request("POST", path, &password);
        assert!(
            reply.starts_with("HTTP/1.1 404") || reply.starts_with("HTTP/1.1 405"),
            "{reply}"
        );
    }
    let guess = json!({"token": "not-a-real-setup-token"}).to_string();
    for path in ["/webauthn/setup/info", "/webauthn/setup/start"] {
        let reply = request("POST", path, &guess);
        assert!(reply.starts_with("HTTP/1.1 400"), "{reply}");
    }
    assert!(
        request("GET", "/", "")
            .to_ascii_lowercase()
            .contains("location: /admin")
    );
    assert!(request("GET", "/admin", "").contains("Finish setup in Portal"));
    fixture.seed_owner();
    assert_eq!(
        fixture.status(),
        json!({"owner_exists":true, "owner_ready":true})
    );
    assert!(request("GET", "/", "").contains("Portal Hub is running"));
    fixture.stop_web();
}

#[test]
fn doctor_sidecar_checks_real_prerequisites_without_requiring_tailscale() {
    let fixture = Fixture::new();
    fixture.seed_owner();
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
    assert_eq!(report["owner_ready"], true);
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
    fixture.seed_owner();
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
    assert_eq!(fixture.status()["owner_ready"], true);
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
    fixture.seed_owner();
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
