//! Whether this computer is connected to Tailscale. A deployed Hub answers
//! only on its private tailnet HTTPS name, so setup checks this before it
//! installs anything rather than failing after the last step.
use std::{path::Path, time::Duration};

/// What the local Tailscale daemon reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalTailnet {
    Connected,
    /// tailscaled answered but is not connected; carries its backend state
    /// (such as `NeedsLogin` or `Stopped`).
    Disconnected(String),
    /// No daemon answered: Tailscale is not installed or not running here, or
    /// runs where Portal cannot reach its socket.
    Unavailable,
}

/// Standard Linux socket locations. Their read-only status is open to every
/// local user, as for `tailscale status`.
const SOCKETS: [&str; 2] = [
    "/run/tailscale/tailscaled.sock",
    "/var/run/tailscale/tailscaled.sock",
];
const RESPONSE_LIMIT: u64 = 1024 * 1024;

pub async fn local_tailnet() -> LocalTailnet {
    for socket in SOCKETS {
        if let Some(state) = backend_state(Path::new(socket)).await {
            return if state == "Running" {
                LocalTailnet::Connected
            } else {
                LocalTailnet::Disconnected(crate::hub::deployment::bounded_text(&state, 32))
            };
        }
    }
    LocalTailnet::Unavailable
}

/// `BackendState` from the daemon's local API, or None when it does not answer.
#[cfg(unix)]
async fn backend_state(socket: &Path) -> Option<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let request = async {
        let mut stream = tokio::net::UnixStream::connect(socket).await.ok()?;
        // HTTP/1.0 makes the daemon close the connection after one plain body.
        stream
            .write_all(
                b"GET /localapi/v0/status?peers=false HTTP/1.0\r\n\
                  Host: local-tailscaled.sock\r\n\
                  Sec-Tailscale: localapi\r\n\r\n",
            )
            .await
            .ok()?;
        let mut response = Vec::new();
        stream
            .take(RESPONSE_LIMIT)
            .read_to_end(&mut response)
            .await
            .ok()?;
        parse_status(&response)
    };
    tokio::time::timeout(Duration::from_secs(3), request)
        .await
        .ok()
        .flatten()
}

#[cfg(not(unix))]
async fn backend_state(_socket: &Path) -> Option<String> {
    None
}

fn parse_status(response: &[u8]) -> Option<String> {
    let end = response.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&response[..end]).ok()?;
    if head.lines().next()?.split_whitespace().nth(1)? != "200" {
        return None;
    }
    let status: serde_json::Value = serde_json::from_slice(&response[end + 4..]).ok()?;
    Some(status.get("BackendState")?.as_str()?.to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A one-shot local API that checks the request and sends `response`.
    async fn ask(response: &'static [u8]) -> Option<String> {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("tailscaled.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let daemon = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let count = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..count]).to_string();
            assert!(request.starts_with("GET /localapi/v0/status?peers=false HTTP/1.0\r\n"));
            assert!(request.contains("\r\nHost: local-tailscaled.sock\r\n"));
            stream.write_all(response).await.unwrap();
        });
        let state = backend_state(&socket).await;
        daemon.await.unwrap();
        state
    }

    #[tokio::test]
    async fn reads_the_backend_state_from_the_local_api() {
        let running = ask(
            b"HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"BackendState\":\"Running\",\"Self\":{}}",
        )
        .await;
        assert_eq!(running.as_deref(), Some("Running"));
        let login = ask(b"HTTP/1.0 200 OK\r\n\r\n{\"BackendState\":\"NeedsLogin\"}").await;
        assert_eq!(login.as_deref(), Some("NeedsLogin"));
    }

    #[tokio::test]
    async fn errors_and_missing_daemons_are_unavailable() {
        let denied = ask(b"HTTP/1.0 403 Forbidden\r\n\r\n{\"BackendState\":\"Running\"}").await;
        assert_eq!(denied, None);
        assert_eq!(ask(b"HTTP/1.0 200 OK\r\n\r\nnot json").await, None);
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            backend_state(&directory.path().join("missing.sock")).await,
            None
        );
    }
}
