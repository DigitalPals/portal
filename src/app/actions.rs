use std::path::PathBuf;
use std::time::Duration;

use futures::stream;
use iced::Task;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::Host;
use crate::message::{EventReceiver, Message, SessionId};
use crate::sftp::SftpClient;
use crate::ssh::{SshClient, SshEvent};

use super::{Portal, View};

impl Portal {
    pub(super) fn set_active_tab(&mut self, tab_id: Uuid) {
        self.active_tab = Some(tab_id);
        if self.sessions.contains_key(&tab_id) {
            self.active_view = View::Terminal(tab_id);
        } else if self.sftp_sessions.contains_key(&tab_id) {
            self.active_view = View::Sftp(tab_id);
        }
    }

    pub(super) fn close_tab(&mut self, tab_id: Uuid) {
        self.tabs.retain(|t| t.id != tab_id);
        self.sessions.remove(&tab_id);
        self.sftp_sessions.remove(&tab_id);

        if self.active_tab == Some(tab_id) {
            if let Some(last_tab) = self.tabs.last() {
                self.set_active_tab(last_tab.id);
            } else {
                self.active_tab = None;
                self.active_view = View::HostGrid;
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

        self.status_message = Some(format!("Connecting to {}...", host.name));

        let (event_tx, event_rx) = mpsc::unbounded_channel::<SshEvent>();

        let ssh_client = SshClient::default();
        let host_clone = host.clone();

        Task::perform(
            async move {
                let result = ssh_client
                    .connect(
                        &host_clone,
                        (80, 24),
                        event_tx,
                        Duration::from_secs(30),
                        None,
                    )
                    .await;

                (session_id, host_clone.name.clone(), result, event_rx)
            },
            |(session_id, host_name, result, event_rx)| match result {
                Ok(ssh_session) => Message::SshConnected {
                    session_id,
                    host_name,
                    ssh_session,
                    event_rx: EventReceiver(Some(event_rx)),
                },
                Err(e) => Message::SshError(format!("Connection failed: {}", e)),
            },
        )
    }

    pub(super) fn connect_sftp(&self, host: &Host) -> Task<Message> {
        let host = host.clone();
        let session_id = Uuid::new_v4();

        let (event_tx, _event_rx) = mpsc::unbounded_channel::<SshEvent>();

        let sftp_client = SftpClient::default();
        let host_clone = host.clone();

        Task::perform(
            async move {
                let result = sftp_client
                    .connect(&host_clone, event_tx, Duration::from_secs(30), None)
                    .await;

                (session_id, host_clone.name.clone(), result)
            },
            |(session_id, host_name, result)| match result {
                Ok(sftp_session) => Message::SftpConnected {
                    session_id,
                    host_name,
                    sftp_session,
                },
                Err(e) => Message::SshError(format!("SFTP connection failed: {}", e)),
            },
        )
    }

    pub(super) fn load_sftp_directory(
        &self,
        session_id: SessionId,
        path: PathBuf,
    ) -> Task<Message> {
        if let Some(state) = self.sftp_sessions.get(&session_id) {
            let sftp = state.sftp_session.clone();
            Task::perform(
                async move { sftp.list_dir(&path).await },
                move |result| {
                    Message::SftpListResult(session_id, result.map_err(|e| e.to_string()))
                },
            )
        } else {
            Task::none()
        }
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
                    _ => Message::Noop,
                },
            );
        }

        Task::none()
    }
}
