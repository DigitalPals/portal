use std::time::Duration;

use futures::stream;
use iced::Task;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::{DetectedOs, Host};
use crate::local_fs::list_local_dir;
use crate::message::{EventReceiver, Message, SessionId, VerificationRequestWrapper};
use crate::sftp::SftpClient;
use crate::ssh::{SshClient, SshEvent};
use crate::views::sftp_view::{PaneId, PaneSource};

use super::{Portal, View};

impl Portal {
    pub(super) fn set_active_tab(&mut self, tab_id: Uuid) {
        self.active_tab = Some(tab_id);
        if self.sessions.contains_key(&tab_id) {
            self.active_view = View::Terminal(tab_id);
        } else if self.dual_sftp_tabs.contains_key(&tab_id) {
            self.active_view = View::DualSftp(tab_id);
        }
    }

    pub(super) fn close_tab(&mut self, tab_id: Uuid) {
        self.tabs.retain(|t| t.id != tab_id);
        self.sessions.remove(&tab_id);
        self.dual_sftp_tabs.remove(&tab_id);

        if self.active_tab == Some(tab_id) {
            if let Some(last_tab) = self.tabs.last() {
                self.set_active_tab(last_tab.id);
            } else {
                self.active_tab = None;
                self.active_view = View::HostGrid;
                self.sidebar_selection = crate::message::SidebarMenuItem::Hosts;
            }
        }
    }

    pub(super) fn close_active_tab(&mut self) {
        if let Some(tab_id) = self.active_tab {
            self.close_tab(tab_id);
        }
    }

    pub(super) fn select_next_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }

        let current_idx = self
            .active_tab
            .and_then(|id| self.tabs.iter().position(|t| t.id == id))
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % self.tabs.len();
        let next_id = self.tabs[next_idx].id;
        self.set_active_tab(next_id);
    }

    pub(super) fn select_prev_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }

        let current_idx = self
            .active_tab
            .and_then(|id| self.tabs.iter().position(|t| t.id == id))
            .unwrap_or(0);
        let prev_idx = if current_idx == 0 {
            self.tabs.len() - 1
        } else {
            current_idx - 1
        };
        let prev_id = self.tabs[prev_idx].id;
        self.set_active_tab(prev_id);
    }

    pub(super) fn connect_to_host(&mut self, host: &Host) -> Task<Message> {
        let host = host.clone();
        let session_id = Uuid::new_v4();
        let host_id = host.id;

        // Detect OS if not already detected, or if it's generic Linux (re-detect to get specific distro)
        let should_detect_os = match &host.detected_os {
            None => true,
            Some(DetectedOs::Linux) => true, // Re-detect generic Linux to get specific distro
            Some(_) => false,
        };

        // Create two channels:
        // 1. For sending events during connection (verification requests)
        // 2. For ongoing session data after connection
        let (event_tx, event_rx) = mpsc::unbounded_channel::<SshEvent>();

        // Start listening for events immediately - this allows us to receive
        // HostKeyVerification events during the connection handshake
        let event_listener = Task::run(
            stream::unfold(event_rx, |mut rx| async move {
                rx.recv().await.map(|event| (event, rx))
            }),
            move |event| match event {
                SshEvent::Data(data) => Message::SshData(session_id, data),
                SshEvent::Disconnected => Message::SshDisconnected(session_id),
                SshEvent::Error(e) => Message::SshError(e),
                SshEvent::HostKeyVerification(request) => {
                    Message::HostKeyVerification(VerificationRequestWrapper(Some(request)))
                }
                SshEvent::Connected => Message::Noop,
            },
        );

        let ssh_client = SshClient::default();
        let host_clone = host.clone();

        // Connection task
        let connect_task = Task::perform(
            async move {
                let result = ssh_client
                    .connect(
                        &host_clone,
                        (80, 24),
                        event_tx,
                        Duration::from_secs(30),
                        None,
                        should_detect_os,
                    )
                    .await;

                (session_id, host_id, host_clone.name.clone(), result)
            },
            |(session_id, host_id, host_name, result)| match result {
                Ok((ssh_session, detected_os)) => {
                    Message::SshConnected {
                        session_id,
                        host_name,
                        ssh_session,
                        // No longer passing event_rx here - it's already being listened to
                        event_rx: EventReceiver(None),
                        host_id,
                        detected_os,
                    }
                }
                Err(e) => Message::SshError(format!("Connection failed: {}", e)),
            },
        );

        // Run both tasks: listener starts immediately, connection proceeds in parallel
        Task::batch([event_listener, connect_task])
    }

    pub(super) fn start_ssh_event_listener(
        &self,
        session_id: SessionId,
        mut event_rx: EventReceiver,
    ) -> Task<Message> {
        if let Some(rx) = event_rx.0.take() {
            return Task::run(
                stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|event| (event, rx)) }),
                move |event| match event {
                    SshEvent::Data(data) => Message::SshData(session_id, data),
                    SshEvent::Disconnected => Message::SshDisconnected(session_id),
                    SshEvent::Error(e) => Message::SshError(e),
                    SshEvent::HostKeyVerification(request) => {
                        Message::HostKeyVerification(VerificationRequestWrapper(Some(request)))
                    }
                    SshEvent::Connected => Message::Noop,
                },
            );
        }

        Task::none()
    }

    /// Load directory contents for a dual-pane SFTP browser pane
    pub(super) fn load_dual_pane_directory(
        &self,
        tab_id: SessionId,
        pane_id: PaneId,
    ) -> Task<Message> {
        if let Some(tab_state) = self.dual_sftp_tabs.get(&tab_id) {
            let pane = tab_state.pane(pane_id);
            let path = pane.current_path.clone();

            match &pane.source {
                PaneSource::Local => {
                    // Load local directory
                    Task::perform(
                        async move { list_local_dir(&path).await },
                        move |result| Message::DualSftpPaneListResult(tab_id, pane_id, result),
                    )
                }
                PaneSource::Remote { session_id, .. } => {
                    // Load remote directory via SFTP
                    if let Some(sftp) = self.sftp_connections.get(session_id) {
                        let sftp = sftp.clone();
                        Task::perform(
                            async move { sftp.list_dir(&path).await },
                            move |result| {
                                Message::DualSftpPaneListResult(
                                    tab_id,
                                    pane_id,
                                    result.map_err(|e| e.to_string()),
                                )
                            },
                        )
                    } else {
                        Task::none()
                    }
                }
            }
        } else {
            Task::none()
        }
    }

    /// Connect to an SFTP host for use in a dual-pane browser
    pub(super) fn connect_sftp_for_pane(
        &mut self,
        tab_id: SessionId,
        pane_id: PaneId,
        host: &Host,
    ) -> Task<Message> {
        let host = host.clone();
        let sftp_session_id = Uuid::new_v4();

        // Store pending connection info for host key verification
        self.pending_dual_sftp_connection = Some((tab_id, pane_id, host.id));

        // Create event channel for SSH events (including host key verification)
        let (event_tx, event_rx) = mpsc::unbounded_channel::<SshEvent>();

        let sftp_client = SftpClient::default();
        let host_name = host.name.clone();

        // Start listening for SSH events (host key verification)
        let event_listener = Task::run(
            futures::stream::unfold(event_rx, |mut rx| async move {
                rx.recv().await.map(|event| (event, rx))
            }),
            move |event| match event {
                SshEvent::HostKeyVerification(request) => {
                    Message::HostKeyVerification(VerificationRequestWrapper(Some(request)))
                }
                _ => Message::Noop,
            },
        );

        // Connection task
        let connect_task = Task::perform(
            async move {
                let result = sftp_client
                    .connect(&host, event_tx, Duration::from_secs(30), None)
                    .await;

                (tab_id, pane_id, sftp_session_id, host_name, result)
            },
            move |(tab_id, pane_id, sftp_session_id, host_name, result)| match result {
                Ok(sftp_session) => {
                    Message::DualSftpConnected {
                        tab_id,
                        pane_id,
                        sftp_session_id,
                        host_name,
                        sftp_session,
                    }
                }
                Err(e) => Message::SshError(format!("SFTP connection failed: {}", e)),
            },
        );

        Task::batch([event_listener, connect_task])
    }
}
