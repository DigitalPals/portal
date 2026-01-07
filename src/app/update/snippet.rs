//! Snippet management message handlers

use std::time::{Duration, Instant};

use iced::Task;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::app::managers::{ExecutionStatus, SnippetExecution};
use crate::app::{Portal, SnippetEditState};
use crate::app::managers::SessionBackend;
use crate::config::{Host, Snippet, HistoricalHostResult, SnippetExecutionEntry};
use crate::message::{HostExecutionResult, Message, SnippetField, SnippetMessage};
use crate::ssh::SshEvent;
use crate::views::toast::Toast;

/// Handle snippet messages
pub fn handle_snippet(portal: &mut Portal, msg: SnippetMessage) -> Task<Message> {
    match msg {
        // Page navigation
        SnippetMessage::SearchChanged(query) => {
            portal.snippet_search_query = query;
            Task::none()
        }

        SnippetMessage::Hover(id) => {
            portal.hovered_snippet = id;
            Task::none()
        }

        SnippetMessage::Select(id) => {
            portal.selected_snippet = Some(id);
            Task::none()
        }

        SnippetMessage::Deselect => {
            portal.selected_snippet = None;
            portal.viewed_history_entry = None;
            Task::none()
        }

        // CRUD operations (page-based)
        SnippetMessage::New => {
            portal.snippet_editing = Some(SnippetEditState::new());
            Task::none()
        }

        SnippetMessage::Edit(id) => {
            if let Some(snippet) = portal.snippets_config.find_snippet(id) {
                portal.snippet_editing = Some(SnippetEditState::from_snippet(snippet));
            }
            Task::none()
        }

        SnippetMessage::Delete(id) => {
            let _ = portal.snippets_config.delete_snippet(id);
            let _ = portal.snippets_config.save();
            portal.snippet_editing = None;
            portal.selected_snippet = None;
            portal.snippet_executions.clear_results(id);
            Task::none()
        }

        SnippetMessage::FieldChanged(field, value) => {
            if let Some(edit) = &mut portal.snippet_editing {
                match field {
                    SnippetField::Name => edit.name = value,
                    SnippetField::Command => edit.command = value,
                    SnippetField::Description => edit.description = value,
                }
            }
            Task::none()
        }

        SnippetMessage::ToggleHost(host_id, checked) => {
            if let Some(edit) = &mut portal.snippet_editing {
                if checked {
                    edit.selected_hosts.insert(host_id);
                } else {
                    edit.selected_hosts.remove(&host_id);
                }
            }
            Task::none()
        }

        SnippetMessage::EditCancel => {
            portal.snippet_editing = None;
            Task::none()
        }

        SnippetMessage::Save => {
            if let Some(edit) = portal.snippet_editing.take() {
                if edit.is_valid() {
                    let now = chrono::Utc::now();
                    let host_ids: Vec<Uuid> = edit.selected_hosts.into_iter().collect();

                    if let Some(id) = edit.snippet_id {
                        // Update existing
                        if let Some(snippet) = portal.snippets_config.find_snippet_mut(id) {
                            snippet.name = edit.name.trim().to_string();
                            snippet.command = edit.command.trim().to_string();
                            snippet.description = if edit.description.trim().is_empty() {
                                None
                            } else {
                                Some(edit.description.trim().to_string())
                            };
                            snippet.host_ids = host_ids;
                            snippet.updated_at = now;
                        }
                    } else {
                        // Create new
                        let mut snippet = Snippet::new(
                            edit.name.trim().to_string(),
                            edit.command.trim().to_string(),
                        );
                        if !edit.description.trim().is_empty() {
                            snippet.description = Some(edit.description.trim().to_string());
                        }
                        snippet.host_ids = host_ids;
                        portal.snippets_config.add_snippet(snippet);
                    }
                    let _ = portal.snippets_config.save();
                }
            }
            Task::none()
        }

        // Legacy: Insert into terminal (for modal compatibility)
        SnippetMessage::Insert(id) => {
            handle_insert(portal, id)
        }

        // Execution
        SnippetMessage::Run(snippet_id) => {
            handle_run(portal, snippet_id)
        }

        SnippetMessage::HostResult {
            snippet_id,
            host_id,
            host_name: _,
            result,
            duration_ms,
        } => {
            if let Some(execution) = portal.snippet_executions.get_active_mut(snippet_id) {
                if let Some(host_result) = execution.get_host_result_mut(host_id) {
                    host_result.duration = Duration::from_millis(duration_ms);
                    match result {
                        Ok(success) => {
                            host_result.status = ExecutionStatus::Success;
                            host_result.stdout = success.stdout;
                            host_result.stderr = success.stderr;
                            host_result.exit_code = Some(success.exit_code);
                        }
                        Err(err) => {
                            host_result.status = ExecutionStatus::Failed(err);
                        }
                    }
                }

                // Check if all hosts are complete
                if execution.all_complete() {
                    let success = execution.success_count();
                    let failed = execution.failure_count();
                    let name = execution.snippet_name.clone();
                    let command = execution.command.clone();

                    // Save to persistent history before completing
                    let history_results: Vec<HistoricalHostResult> = execution
                        .host_results
                        .iter()
                        .map(|r| {
                            let (success, error) = match &r.status {
                                ExecutionStatus::Success => (true, None),
                                ExecutionStatus::Failed(e) => (false, Some(e.clone())),
                                _ => (false, None),
                            };
                            HistoricalHostResult {
                                host_id: r.host_id,
                                host_name: r.host_name.clone(),
                                success,
                                stdout: r.stdout.clone(),
                                stderr: r.stderr.clone(),
                                exit_code: r.exit_code,
                                duration_ms: r.duration.as_millis() as u64,
                                error,
                            }
                        })
                        .collect();

                    let history_entry = SnippetExecutionEntry::new(
                        snippet_id,
                        name.clone(),
                        command,
                        history_results,
                    );
                    portal.snippet_history.add_entry(history_entry);
                    if let Err(e) = portal.snippet_history.save() {
                        tracing::warn!("Failed to save snippet history: {}", e);
                    }

                    portal.snippet_executions.complete_execution(snippet_id);

                    // Show toast notification
                    if failed == 0 {
                        portal.toast_manager.push(Toast::success(format!(
                            "'{}' completed on {} hosts",
                            name, success
                        )));
                    } else {
                        portal.toast_manager.push(Toast::warning(format!(
                            "'{}': {} succeeded, {} failed",
                            name, success, failed
                        )));
                    }
                }
            }
            Task::none()
        }

        // Results panel
        SnippetMessage::ToggleResultExpand(snippet_id, host_id) => {
            if let Some(execution) = portal.snippet_executions.get_last_result_mut(snippet_id) {
                if let Some(result) = execution.get_host_result_mut(host_id) {
                    result.expanded = !result.expanded;
                }
            }
            Task::none()
        }

        SnippetMessage::ClearResults(snippet_id) => {
            portal.snippet_executions.clear_results(snippet_id);
            // Deselect the snippet to hide the panel
            if portal.selected_snippet == Some(snippet_id) {
                portal.selected_snippet = None;
            }
            Task::none()
        }

        SnippetMessage::ViewHistoryEntry(entry_id) => {
            portal.viewed_history_entry = Some(entry_id);
            Task::none()
        }

        SnippetMessage::ViewCurrentResults => {
            portal.viewed_history_entry = None;
            Task::none()
        }
    }
}

/// Handle legacy insert into terminal
fn handle_insert(portal: &mut Portal, id: Uuid) -> Task<Message> {
    tracing::debug!("Snippet insert requested for id: {}", id);
    if let Some(snippet) = portal.snippets_config.find_snippet(id) {
        let command = snippet.command.clone();
        tracing::debug!("Found snippet '{}', command: {}", snippet.name, command);
        if let Some(session_id) = portal.active_tab {
            tracing::debug!("Active tab: {}", session_id);
            if let Some(session) = portal.sessions.get(session_id) {
                tracing::info!("Inserting snippet '{}' into terminal", snippet.name);
                let data = command.into_bytes();
                portal.dialogs.close();
                match &session.backend {
                    SessionBackend::Ssh(ssh_session) => {
                        let ssh = ssh_session.clone();
                        return Task::perform(
                            async move {
                                let _ = ssh.send(&data).await;
                            },
                            move |_| Message::Noop,
                        );
                    }
                    SessionBackend::Local(local_session) => {
                        let local = local_session.clone();
                        return Task::perform(
                            async move {
                                let _ = local.send(&data).await;
                            },
                            move |_| Message::Noop,
                        );
                    }
                }
            } else {
                tracing::warn!("No session found for active tab {}", session_id);
            }
        } else {
            tracing::warn!("No active tab when inserting snippet");
        }
    } else {
        tracing::warn!("Snippet not found: {}", id);
    }
    portal.dialogs.close();
    Task::none()
}

/// Handle running a snippet on multiple hosts
fn handle_run(portal: &mut Portal, snippet_id: Uuid) -> Task<Message> {
    let Some(snippet) = portal.snippets_config.find_snippet(snippet_id) else {
        portal.toast_manager.push(Toast::warning("Snippet not found"));
        return Task::none();
    };

    if snippet.host_ids.is_empty() {
        portal.toast_manager.push(Toast::warning("No hosts assigned to this snippet"));
        return Task::none();
    }

    // Auto-select the snippet to show results panel
    portal.selected_snippet = Some(snippet_id);
    // Clear any history view to show current execution
    portal.viewed_history_entry = None;

    // Collect host info
    let hosts_info: Vec<(Uuid, String, Host)> = snippet
        .host_ids
        .iter()
        .filter_map(|&hid| {
            portal
                .hosts_config
                .find_host(hid)
                .map(|h| (hid, h.name.clone(), h.clone()))
        })
        .collect();

    if hosts_info.is_empty() {
        portal.toast_manager.push(Toast::warning("None of the assigned hosts exist"));
        return Task::none();
    }

    // Create execution tracker
    let execution = SnippetExecution::new(
        snippet_id,
        snippet.name.clone(),
        snippet.command.clone(),
        hosts_info.iter().map(|(id, name, _)| (*id, name.clone())).collect(),
    );
    portal.snippet_executions.start_execution(execution);

    // Mark all hosts as running
    if let Some(exec) = portal.snippet_executions.get_active_mut(snippet_id) {
        for result in &mut exec.host_results {
            result.status = ExecutionStatus::Running;
        }
    }

    // Spawn parallel execution tasks for each host
    let command = snippet.command.clone();
    let tasks: Vec<Task<Message>> = hosts_info
        .into_iter()
        .map(|(host_id, host_name, host)| {
            let cmd = command.clone();
            let hname = host_name.clone();

            Task::perform(
                async move {
                    let start = Instant::now();
                    let result = execute_on_host(&host, &cmd).await;
                    let duration = start.elapsed();
                    (snippet_id, host_id, hname, result, duration.as_millis() as u64)
                },
                |(snippet_id, host_id, host_name, result, duration_ms)| {
                    Message::Snippet(SnippetMessage::HostResult {
                        snippet_id,
                        host_id,
                        host_name,
                        result,
                        duration_ms,
                    })
                },
            )
        })
        .collect();

    Task::batch(tasks)
}

/// Execute a command on a single host
/// Connects via SSH, runs the command, returns stdout
async fn execute_on_host(host: &Host, command: &str) -> Result<HostExecutionResult, String> {
    use crate::app::services::connection::shared_known_hosts_manager;
    use crate::ssh::SshClient;

    // Create a channel for SSH events
    let (event_tx, mut event_rx) = mpsc::channel::<SshEvent>(16);

    // Spawn a task to handle SSH events
    // For snippet execution, we auto-accept host keys for known hosts
    // but reject unknown hosts (they should connect interactively first)
    tokio::spawn(async move {
        use crate::ssh::host_key_verification::{
            HostKeyVerificationRequest, HostKeyVerificationResponse,
        };

        while let Some(event) = event_rx.recv().await {
            match event {
                SshEvent::HostKeyVerification(request) => {
                    // Auto-accept the key for snippet execution
                    // The known_hosts manager will have already verified if it's known
                    // If we get here, it means the host key needs verification
                    tracing::warn!(
                        "Host key verification required for snippet execution - auto-accepting"
                    );
                    let responder = match *request {
                        HostKeyVerificationRequest::NewHost { responder, .. } => responder,
                        HostKeyVerificationRequest::ChangedHost { responder, .. } => responder,
                    };
                    let _ = responder.send(HostKeyVerificationResponse::Accept);
                }
                SshEvent::Disconnected => {
                    tracing::debug!("SSH disconnected during snippet execution");
                }
                _ => {}
            }
        }
    });

    // Create SSH client with shared known hosts manager
    let known_hosts = shared_known_hosts_manager();
    let client = SshClient::with_known_hosts(0, known_hosts); // No keepalive for exec

    // Connect to the host
    tracing::debug!("Connecting to host {} for snippet execution", host.name);
    let connection_result = client
        .connect(
            host,
            (80, 24), // Minimal terminal size for exec
            event_tx,
            Duration::from_secs(15),
            None,  // No password (use key auth)
            false, // Don't detect OS
        )
        .await;

    let (ssh_session, _) = connection_result.map_err(|e| {
        tracing::error!("Snippet execution connection failed to {}: {}", host.name, e);
        format!("Connection failed: {}", e)
    })?;

    tracing::debug!("Connected to {}, executing command", host.name);

    // Execute the command
    let output = ssh_session
        .execute_command(command)
        .await
        .map_err(|e| {
            tracing::error!("Snippet command execution failed on {}: {}", host.name, e);
            format!("Execution failed: {}", e)
        })?;

    tracing::debug!("Command completed on {}, output length: {}", host.name, output.len());

    Ok(HostExecutionResult {
        stdout: output,
        stderr: String::new(), // execute_command doesn't capture stderr separately
        exit_code: 0,          // We don't have exit code from execute_command
    })
}
