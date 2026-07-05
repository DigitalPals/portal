//! ProxyJump / bastion host tunneling.
//!
//! Resolves a host's jump chain from configuration (with cycle and depth
//! guards) and establishes the tunneled transport: each hop is a full SSH
//! connection (normal auth + host key verification), and the next hop's
//! transport is a `direct-tcpip` channel opened through the previous hop.

use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::task::{Context, Poll};
use std::time::Duration;

use russh::client;
use russh::{Channel, ChannelStream};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};
use tokio::time::timeout;
use uuid::Uuid;

use crate::config::{Host, Protocol};
use crate::error::SshError;
use crate::security_log;

use super::SshEvent;
use super::auth::ResolvedAuth;
use super::auth_flow::{self, AuthContext};
use super::connection_pool::{SshConnection, SshConnectionKey};
use super::handler::ClientHandler;
use super::known_hosts::KnownHostsManager;
use super::passphrase_cache;
use super::shared_connection_pool;

/// Maximum number of chained jump hosts.
pub const MAX_JUMP_DEPTH: usize = 5;

/// Errors resolving a jump chain from configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JumpChainError {
    #[error("Jump host chain for '{0}' exceeds the maximum depth of {MAX_JUMP_DEPTH}")]
    TooDeep(String),
    #[error("Jump host cycle detected in the chain for '{0}'")]
    Cycle(String),
    #[error("Jump host configured for '{0}' was not found")]
    NotFound(String),
    #[error("Jump host '{0}' is not an SSH host")]
    NotSsh(String),
}

/// Resolve the ordered jump chain for `target`, outermost hop first.
///
/// Returns an empty vector for hosts without a jump host. Guards against
/// cycles (visited set) and unbounded depth.
pub fn resolve_jump_chain(hosts: &[Host], target: &Host) -> Result<Vec<Host>, JumpChainError> {
    let mut chain: Vec<Host> = Vec::new();
    let mut visited: HashSet<Uuid> = HashSet::new();
    visited.insert(target.id);

    let mut current = target.jump_host_id;
    let mut current_name = target.name.clone();

    while let Some(jump_id) = current {
        let hop = hosts
            .iter()
            .find(|h| h.id == jump_id)
            .ok_or_else(|| JumpChainError::NotFound(current_name.clone()))?;

        if hop.protocol != Protocol::Ssh {
            return Err(JumpChainError::NotSsh(hop.name.clone()));
        }
        if !visited.insert(hop.id) {
            return Err(JumpChainError::Cycle(target.name.clone()));
        }
        if chain.len() >= MAX_JUMP_DEPTH {
            return Err(JumpChainError::TooDeep(target.name.clone()));
        }

        chain.push(hop.clone());
        current = hop.jump_host_id;
        current_name = hop.name.clone();
    }

    // Walked target -> innermost hop -> ... -> outermost; connect order is
    // outermost first.
    chain.reverse();
    Ok(chain)
}

/// A human-readable description of a jump chain ("via bastion -> dmz").
pub fn describe_chain(chain: &[Host]) -> String {
    chain
        .iter()
        .map(|hop| hop.name.as_str())
        .collect::<Vec<_>>()
        .join(" -> ")
}

/// Stable pool-key discriminator for connections established through a chain.
pub fn chain_via_key(chain: &[Host]) -> String {
    chain
        .iter()
        .map(|hop| hop.id.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Transport stream for an SSH connection: direct TCP or tunneled through a
/// `direct-tcpip` channel of a jump host connection.
pub enum TunnelStream {
    Tcp(TcpStream),
    Channel(ChannelStream<client::Msg>),
}

impl AsyncRead for TunnelStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            TunnelStream::Tcp(stream) => Pin::new(stream).poll_read(cx, buf),
            TunnelStream::Channel(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for TunnelStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            TunnelStream::Tcp(stream) => Pin::new(stream).poll_write(cx, buf),
            TunnelStream::Channel(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            TunnelStream::Tcp(stream) => Pin::new(stream).poll_flush(cx),
            TunnelStream::Channel(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            TunnelStream::Tcp(stream) => Pin::new(stream).poll_shutdown(cx),
            TunnelStream::Channel(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

/// Parameters shared by all hop connections of one tunnel.
pub struct TunnelParams {
    pub config: Arc<client::Config>,
    pub known_hosts: Arc<Mutex<KnownHostsManager>>,
    pub event_tx: mpsc::Sender<SshEvent>,
    pub connect_timeout: Duration,
}

impl TunnelParams {
    /// Build tunnel parameters with the standard client configuration
    /// (matching the keepalive/inactivity settings used for terminal and
    /// SFTP connections). Used by callers that open a tunnel without going
    /// through `SshClient`, e.g. VNC-over-SSH.
    pub fn new(
        known_hosts: Arc<Mutex<KnownHostsManager>>,
        event_tx: mpsc::Sender<SshEvent>,
        connect_timeout: Duration,
    ) -> Self {
        let config = client::Config {
            inactivity_timeout: Some(Duration::from_secs(3600)),
            keepalive_interval: Some(Duration::from_secs(60)),
            keepalive_max: 3,
            ..Default::default()
        };
        Self {
            config: Arc::new(config),
            known_hosts,
            event_tx,
            connect_timeout,
        }
    }
}

fn hop_error(hop: &Host, reason: String) -> SshError {
    SshError::ConnectionFailed {
        host: hop.hostname.clone(),
        port: hop.port,
        reason: format!("jump host '{}' failed: {}", hop.name, reason),
    }
}

/// A stream tunneled to the target through a chain of jump hosts.
///
/// The last hop's connection is included so callers can keep the tunnel
/// transport alive: the connection pool only holds weak references, and hop
/// connections keep their own parents alive transitively.
pub struct TunneledStream {
    pub stream: TunnelStream,
    pub last_hop: Arc<SshConnection>,
}

/// Establish (or reuse) SSH connections for every hop in `chain` and return
/// a stream tunneled to `target_host:target_port` through the last hop.
///
/// `chain` must be non-empty; host key verification and full authentication
/// (including keyboard-interactive prompts) run for every hop.
pub async fn open_tunneled_stream(
    params: &TunnelParams,
    chain: &[Host],
    target_host: &str,
    target_port: u16,
) -> Result<TunneledStream, SshError> {
    let last_hop = connect_chain(params, chain).await?;
    let hop = chain.last().map(|h| h.name.clone()).unwrap_or_default();

    let stream = open_direct_tcpip(&last_hop, target_host, target_port)
        .await
        .map_err(|e| SshError::ConnectionFailed {
            host: target_host.to_string(),
            port: target_port,
            reason: format!("tunnel from jump host '{}' failed: {}", hop, e),
        })?;

    Ok(TunneledStream { stream, last_hop })
}

/// Connect (or reuse pooled connections) along the chain; returns the last
/// hop's connection.
async fn connect_chain(
    params: &TunnelParams,
    chain: &[Host],
) -> Result<Arc<SshConnection>, SshError> {
    let pool = shared_connection_pool();
    let mut prev: Option<Arc<SshConnection>> = None;

    for (index, hop) in chain.iter().enumerate() {
        let via = chain_via_key(&chain[..index]);
        let username = hop.effective_username();
        let key = SshConnectionKey::with_via(&hop.hostname, hop.port, &username, &via);

        // Reuse a live pooled connection when possible.
        if let Some(conn) = pool.get(&key).await {
            let closed = {
                let handle = conn.handle();
                let guard = handle.lock().await;
                guard.is_closed()
            };
            if closed {
                pool.invalidate_if_matches(&key, &conn).await;
            } else {
                prev = Some(conn);
                continue;
            }
        }

        // Open the transport for this hop: direct TCP for the first hop,
        // a direct-tcpip channel through the previous hop otherwise.
        let stream = match &prev {
            None => {
                let addr = format!("{}:{}", hop.hostname, hop.port);
                let stream = timeout(params.connect_timeout, TcpStream::connect(&addr))
                    .await
                    .map_err(|_| hop_error(hop, format!("connection to {} timed out", addr)))?
                    .map_err(|e| hop_error(hop, e.to_string()))?;
                TunnelStream::Tcp(stream)
            }
            Some(conn) => open_direct_tcpip(conn, &hop.hostname, hop.port)
                .await
                .map_err(|e| hop_error(hop, format!("tunnel channel failed: {}", e)))?,
        };

        let handler = ClientHandler::new(
            hop.hostname.clone(),
            hop.port,
            params.known_hosts.clone(),
            params.event_tx.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(HashMap::new())),
        );

        // Host key verification happens inside the handshake and can wait on
        // a user dialog, so no tight timeout here — the dialog wait itself is
        // bounded.
        let mut handle = client::connect_stream(params.config.clone(), stream, handler)
            .await
            .map_err(|e| hop_error(hop, e.to_string()))?;

        // Resolve auth for the hop. Passwords are never pre-collected for
        // jump hosts; password-auth hops degrade to keyboard-interactive,
        // which lets the server prompt through the auth dialog. Encrypted
        // key passphrases are honored from the in-memory passphrase cache.
        let resolved = resolve_hop_auth(hop).await.map_err(|e| match e {
            SshError::KeyFilePassphraseRequired(path) | SshError::KeyFilePassphraseInvalid(path) => {
                hop_error(
                    hop,
                    format!(
                        "key {} requires a passphrase — connect to '{}' directly first to unlock it",
                        path.display(),
                        hop.name
                    ),
                )
            }
            other => hop_error(hop, other.to_string()),
        })?;

        auth_flow::authenticate(
            &mut handle,
            AuthContext {
                hostname: &hop.hostname,
                port: hop.port,
                username: &username,
                event_tx: &params.event_tx,
            },
            resolved,
        )
        .await
        .map_err(|e| hop_error(hop, e.to_string()))?;

        let conn = SshConnection::new_via(
            handle,
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::from(hop.hostname.clone()),
            hop.port,
            prev.take(),
        );
        security_log::log_ssh_connect(&hop.hostname, hop.port, &username);
        pool.put(key, conn.clone()).await;
        prev = Some(conn);
    }

    prev.ok_or_else(|| SshError::ConnectionFailed {
        host: String::new(),
        port: 0,
        reason: "empty jump chain".to_string(),
    })
}

async fn resolve_hop_auth(hop: &Host) -> Result<ResolvedAuth, SshError> {
    use crate::config::AuthMethod;

    if matches!(hop.auth, AuthMethod::Password) {
        // No pre-collected password for jump hosts: let the server prompt
        // via keyboard-interactive instead.
        return Ok(ResolvedAuth::KeyboardInteractive);
    }

    let passphrase = match &hop.auth {
        AuthMethod::PublicKey {
            key_path: Some(path),
            ..
        } => {
            let expanded = crate::config::paths::expand_tilde(&path.to_string_lossy());
            passphrase_cache::shared_cache().get(&expanded)
        }
        _ => None,
    };

    ResolvedAuth::resolve(&hop.auth, None, passphrase).await
}

async fn open_direct_tcpip(
    conn: &SshConnection,
    host: &str,
    port: u16,
) -> Result<TunnelStream, SshError> {
    let channel: Channel<client::Msg> = {
        let handle = conn.handle();
        let guard = handle.lock().await;
        guard
            .channel_open_direct_tcpip(host, port as u32, "127.0.0.1", 0)
            .await
            .map_err(|e| SshError::Channel(e.to_string()))?
    };

    Ok(TunnelStream::Channel(channel.into_stream()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuthMethod;
    use crate::config::hosts::HubRouting;

    fn test_host(name: &str, jump: Option<Uuid>) -> Host {
        let now = chrono::Utc::now();
        Host {
            id: Uuid::new_v4(),
            name: name.to_string(),
            hostname: format!("{}.example.test", name),
            port: 22,
            username: "root".to_string(),
            protocol: Protocol::Ssh,
            vnc_port: None,
            vnc_password_id: None,
            vnc_via_ssh_host_id: None,
            allow_cleartext_vnc: false,
            auth: AuthMethod::Agent,
            agent_forwarding: false,
            port_forwards: Vec::new(),
            hub_routing: HubRouting::Auto,
            jump_host_id: jump,
            group_id: None,
            notes: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            detected_os: None,
            last_connected: None,
        }
    }

    #[test]
    fn direct_host_has_empty_chain() {
        let target = test_host("target", None);
        let chain = resolve_jump_chain(std::slice::from_ref(&target), &target).unwrap();
        assert!(chain.is_empty());
    }

    #[test]
    fn single_jump_resolves() {
        let bastion = test_host("bastion", None);
        let target = test_host("target", Some(bastion.id));
        let hosts = vec![bastion.clone(), target.clone()];

        let chain = resolve_jump_chain(&hosts, &target).unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].id, bastion.id);
    }

    #[test]
    fn chained_jumps_resolve_outermost_first() {
        let outer = test_host("outer", None);
        let inner = test_host("inner", Some(outer.id));
        let target = test_host("target", Some(inner.id));
        let hosts = vec![outer.clone(), inner.clone(), target.clone()];

        let chain = resolve_jump_chain(&hosts, &target).unwrap();
        let ids: Vec<Uuid> = chain.iter().map(|h| h.id).collect();
        assert_eq!(ids, vec![outer.id, inner.id]);
    }

    #[test]
    fn cycle_is_detected() {
        let mut a = test_host("a", None);
        let b = test_host("b", Some(a.id));
        a.jump_host_id = Some(b.id);
        let target = test_host("target", Some(a.id));
        let hosts = vec![a.clone(), b.clone(), target.clone()];

        let error = resolve_jump_chain(&hosts, &target).unwrap_err();
        assert!(matches!(error, JumpChainError::Cycle(_)));
    }

    #[test]
    fn self_reference_is_detected_as_cycle() {
        let mut target = test_host("target", None);
        target.jump_host_id = Some(target.id);
        let hosts = vec![target.clone()];

        let error = resolve_jump_chain(&hosts, &target).unwrap_err();
        assert!(matches!(error, JumpChainError::Cycle(_)));
    }

    #[test]
    fn depth_limit_is_enforced() {
        // Build a linear chain longer than MAX_JUMP_DEPTH.
        let mut hosts: Vec<Host> = Vec::new();
        let mut prev: Option<Uuid> = None;
        for i in 0..(MAX_JUMP_DEPTH + 2) {
            let host = test_host(&format!("hop{}", i), prev);
            prev = Some(host.id);
            hosts.push(host);
        }
        let target = test_host("target", prev);
        hosts.push(target.clone());

        let error = resolve_jump_chain(&hosts, &target).unwrap_err();
        assert!(matches!(error, JumpChainError::TooDeep(_)));
    }

    #[test]
    fn missing_jump_host_is_an_error() {
        let target = test_host("target", Some(Uuid::new_v4()));
        let error = resolve_jump_chain(std::slice::from_ref(&target), &target).unwrap_err();
        assert!(matches!(error, JumpChainError::NotFound(_)));
    }

    #[test]
    fn vnc_jump_host_is_rejected() {
        let mut bastion = test_host("bastion", None);
        bastion.protocol = Protocol::Vnc;
        let target = test_host("target", Some(bastion.id));
        let hosts = vec![bastion, target.clone()];

        let error = resolve_jump_chain(&hosts, &target).unwrap_err();
        assert!(matches!(error, JumpChainError::NotSsh(_)));
    }

    #[test]
    fn describe_chain_joins_names() {
        let a = test_host("bastion", None);
        let b = test_host("dmz", None);
        assert_eq!(describe_chain(&[a, b]), "bastion -> dmz");
    }
}
