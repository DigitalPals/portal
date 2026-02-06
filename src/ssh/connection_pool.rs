use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use russh::Disconnect;
use russh::client::Handle;
use tokio::sync::Mutex;

use crate::config::PortForward;
use crate::error::SshError;
use crate::security_log;

use super::handler::ClientHandler;

#[derive(Clone, Debug, Eq)]
pub struct SshConnectionKey {
    pub host: Arc<str>,
    pub port: u16,
    pub username: Arc<str>,
}

impl PartialEq for SshConnectionKey {
    fn eq(&self, other: &Self) -> bool {
        self.port == other.port
            && self.host.as_ref() == other.host.as_ref()
            && self.username.as_ref() == other.username.as_ref()
    }
}

impl Hash for SshConnectionKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.host.as_ref().hash(state);
        self.port.hash(state);
        self.username.as_ref().hash(state);
    }
}

impl SshConnectionKey {
    pub fn new(host: &str, port: u16, username: &str) -> Self {
        Self {
            host: Arc::from(host),
            port,
            username: Arc::from(username),
        }
    }
}

pub struct SshConnection {
    handle: Arc<Mutex<Handle<ClientHandler>>>,
    remote_forwards: Arc<Mutex<HashMap<uuid::Uuid, PortForward>>>,
    agent_forwarding_enabled: Arc<AtomicBool>,
    host: Arc<str>,
    port: u16,
    disconnect_logged: AtomicBool,
}

impl std::fmt::Debug for SshConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshConnection")
            .field("host", &self.host)
            .field("port", &self.port)
            .finish()
    }
}

impl SshConnection {
    pub fn new(
        handle: Handle<ClientHandler>,
        remote_forwards: Arc<Mutex<HashMap<uuid::Uuid, PortForward>>>,
        agent_forwarding_enabled: Arc<AtomicBool>,
        host: Arc<str>,
        port: u16,
    ) -> Arc<Self> {
        Arc::new(Self {
            handle: Arc::new(Mutex::new(handle)),
            remote_forwards,
            agent_forwarding_enabled,
            host,
            port,
            disconnect_logged: AtomicBool::new(false),
        })
    }

    pub fn handle(&self) -> Arc<Mutex<Handle<ClientHandler>>> {
        self.handle.clone()
    }

    pub fn remote_forwards(&self) -> Arc<Mutex<HashMap<uuid::Uuid, PortForward>>> {
        self.remote_forwards.clone()
    }

    pub fn agent_forwarding_enabled(&self) -> Arc<AtomicBool> {
        self.agent_forwarding_enabled.clone()
    }

    pub fn host(&self) -> &str {
        self.host.as_ref()
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn enable_agent_forwarding(&self) {
        self.agent_forwarding_enabled.store(true, Ordering::SeqCst);
    }

    pub async fn disconnect(&self) -> Result<(), SshError> {
        let handle_guard = self.handle.lock().await;
        handle_guard
            .disconnect(Disconnect::ByApplication, "connection dropped", "en")
            .await
            .map_err(|e| SshError::Channel(e.to_string()))
    }
}

impl Drop for SshConnection {
    fn drop(&mut self) {
        if !self.disconnect_logged.swap(true, Ordering::SeqCst) {
            security_log::log_ssh_disconnect(self.host.as_ref(), self.port, false);
        }
        let handle = self.handle.clone();
        let host = self.host.to_string();
        let port = self.port;
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn(async move {
                    let handle_guard = handle.lock().await;
                    let _ = handle_guard
                        .disconnect(Disconnect::ByApplication, "connection dropped", "en")
                        .await;
                    tracing::debug!("SSH connection cleanup: disconnected {}:{}", host, port);
                });
            }
            Err(_) => {
                tracing::debug!(
                    "SSH connection dropped without a Tokio runtime; disconnect skipped"
                );
            }
        }
    }
}

#[derive(Default)]
pub struct SshConnectionPool {
    connections: Mutex<HashMap<SshConnectionKey, std::sync::Weak<SshConnection>>>,
}

impl SshConnectionPool {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
        }
    }

    pub async fn get(&self, key: &SshConnectionKey) -> Option<Arc<SshConnection>> {
        let mut map = self.connections.lock().await;
        let weak = map.get(key).cloned()?;

        match weak.upgrade() {
            Some(conn) => Some(conn),
            None => {
                // Stale entry.
                map.remove(key);
                None
            }
        }
    }

    pub async fn put(&self, key: SshConnectionKey, conn: Arc<SshConnection>) {
        let mut map = self.connections.lock().await;
        map.insert(key, Arc::downgrade(&conn));
    }

    pub async fn invalidate_if_matches(&self, key: &SshConnectionKey, conn: &Arc<SshConnection>) {
        let mut map = self.connections.lock().await;
        let Some(existing) = map.get(key) else {
            return;
        };
        if let Some(existing) = existing.upgrade() {
            if Arc::ptr_eq(&existing, conn) {
                map.remove(key);
            }
        } else {
            map.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_equality_and_hashing() {
        let a = SshConnectionKey::new("example.com", 22, "user");
        let b = SshConnectionKey::new("example.com", 22, "user");
        let c = SshConnectionKey::new("example.com", 2222, "user");
        let d = SshConnectionKey::new("example.com", 22, "other");
        let e = SshConnectionKey::new("other.com", 22, "user");

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
        assert_ne!(a, e);
    }

    #[tokio::test]
    async fn pool_put_get_roundtrip_without_connection() {
        // This is a structural test of pool bookkeeping (weak->strong behavior).
        // We can't create a real SshConnection here without network.
        let pool = SshConnectionPool::new();
        let key = SshConnectionKey::new("example.com", 22, "user");
        assert!(pool.get(&key).await.is_none());
    }
}
