//! Portal Proxy PTY session management.
//!
//! The proxy transport is intentionally backed by the local OpenSSH client for
//! the prototype. OpenSSH handles proxy authentication, Tailscale-only routing,
//! PTY allocation, and SSH agent forwarding; Portal handles terminal rendering.

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::ffi::OsString;
use std::io::{Read, Write};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::Host;
use crate::config::paths;
use crate::config::settings::PortalProxySettings;
use crate::error::LocalError;

#[derive(Debug)]
pub enum ProxyEvent {
    Data(Vec<u8>),
    Disconnected { clean: bool },
}

enum ProxyCommand {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}

#[derive(Debug)]
pub struct ProxySession {
    command_tx: mpsc::Sender<ProxyCommand>,
    child_killer: Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>,
}

impl ProxySession {
    pub fn spawn(
        settings: &PortalProxySettings,
        host: &Host,
        session_id: Uuid,
        cols: u16,
        rows: u16,
        event_tx: mpsc::Sender<ProxyEvent>,
    ) -> Result<Self, LocalError> {
        let proxy_host = settings.host.trim();
        if proxy_host.is_empty() {
            return Err(LocalError::SpawnFailed(
                "Portal Proxy host is not configured".to_string(),
            ));
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| LocalError::PtyCreation(e.to_string()))?;

        let mut argv = vec![OsString::from("ssh")];
        argv.push(OsString::from("-F"));
        argv.push(OsString::from("/dev/null"));
        argv.push(OsString::from("-tt"));
        argv.push(OsString::from("-A"));
        argv.push(OsString::from("-o"));
        argv.push(OsString::from("StrictHostKeyChecking=accept-new"));
        if let Some(config_dir) = paths::config_dir() {
            argv.push(OsString::from("-o"));
            argv.push(OsString::from(format!(
                "UserKnownHostsFile={}",
                config_dir.join("portal_proxy_known_hosts").display()
            )));
        }
        argv.push(OsString::from("-p"));
        argv.push(OsString::from(settings.port.to_string()));
        argv.push(OsString::from("-l"));
        argv.push(OsString::from(settings.username.clone()));
        if let Some(identity) = settings
            .identity_file
            .as_ref()
            .filter(|path| !path.as_os_str().is_empty())
        {
            argv.push(OsString::from("-i"));
            argv.push(identity.as_os_str().to_os_string());
        }
        argv.push(OsString::from(proxy_host));
        argv.push(OsString::from("portal-proxy"));
        argv.push(OsString::from("attach"));
        argv.push(OsString::from("--session-id"));
        argv.push(OsString::from(session_id.to_string()));
        argv.push(OsString::from("--target-host"));
        argv.push(OsString::from(host.hostname.clone()));
        argv.push(OsString::from("--target-port"));
        argv.push(OsString::from(host.port.to_string()));
        argv.push(OsString::from("--target-user"));
        argv.push(OsString::from(host.effective_username()));
        argv.push(OsString::from("--cols"));
        argv.push(OsString::from(cols.to_string()));
        argv.push(OsString::from("--rows"));
        argv.push(OsString::from(rows.to_string()));

        let mut cmd = CommandBuilder::from_argv(argv);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "Portal");

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| LocalError::SpawnFailed(e.to_string()))?;
        let child_killer = child.clone_killer();

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| LocalError::Io(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| LocalError::Io(e.to_string()))?;
        let master = pair.master;

        let (command_tx, command_rx) = mpsc::channel::<ProxyCommand>(256);
        Self::spawn_io_task(reader, writer, master, command_rx, event_tx.clone());
        std::thread::spawn(move || match child.wait() {
            Ok(status) => {
                let _ = event_tx.blocking_send(ProxyEvent::Disconnected {
                    clean: status.success(),
                });
            }
            Err(error) => {
                tracing::error!("Portal Proxy ssh wait error: {}", error);
                let _ = event_tx.blocking_send(ProxyEvent::Disconnected { clean: false });
            }
        });

        Ok(Self {
            command_tx,
            child_killer: Some(child_killer),
        })
    }

    fn spawn_io_task(
        mut reader: Box<dyn Read + Send>,
        mut writer: Box<dyn Write + Send>,
        master: Box<dyn portable_pty::MasterPty + Send>,
        mut command_rx: mpsc::Receiver<ProxyCommand>,
        event_tx: mpsc::Sender<ProxyEvent>,
    ) {
        let event_tx_reader = event_tx.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        break;
                    }
                    Ok(n) => {
                        let _ = event_tx_reader.blocking_send(ProxyEvent::Data(buf[..n].to_vec()));
                    }
                    Err(error) => {
                        tracing::error!("Portal Proxy PTY read error: {}", error);
                        let _ = event_tx_reader
                            .blocking_send(ProxyEvent::Disconnected { clean: false });
                        break;
                    }
                }
            }
        });

        std::thread::spawn(move || {
            let _master = master;
            while let Some(cmd) = command_rx.blocking_recv() {
                match cmd {
                    ProxyCommand::Data(data) => {
                        if let Err(error) = writer.write_all(&data) {
                            tracing::error!("Portal Proxy PTY write error: {}", error);
                            break;
                        }
                        let _ = writer.flush();
                    }
                    ProxyCommand::Resize { cols, rows } => {
                        if let Err(error) = _master.resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        }) {
                            tracing::error!("Portal Proxy PTY resize error: {}", error);
                        }
                    }
                }
            }

            let _ = event_tx.blocking_send(ProxyEvent::Disconnected { clean: false });
        });
    }

    pub async fn send(&self, data: &[u8]) -> Result<(), LocalError> {
        self.command_tx
            .send(ProxyCommand::Data(data.to_vec()))
            .await
            .map_err(|e| LocalError::Io(e.to_string()))
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), LocalError> {
        self.command_tx
            .send(ProxyCommand::Resize { cols, rows })
            .await
            .map_err(|e| LocalError::Io(e.to_string()))
    }
}

impl Drop for ProxySession {
    fn drop(&mut self) {
        tracing::debug!("Portal Proxy session cleanup: detaching local ssh process");
        let (replacement_tx, _replacement_rx) = mpsc::channel(1);
        let _ = std::mem::replace(&mut self.command_tx, replacement_tx);
        if let Some(mut killer) = self.child_killer.take() {
            if let Err(error) = killer.kill() {
                tracing::debug!("Failed to kill Portal Proxy ssh process: {}", error);
            }
        }
    }
}
