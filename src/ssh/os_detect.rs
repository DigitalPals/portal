//! OS detection for remote SSH hosts
//!
//! Detects the operating system of a remote host by executing `uname -s`
//! and parsing `/etc/os-release` for Linux distribution identification.

use std::time::Duration;

use russh::ChannelMsg;
use tokio::time::timeout;

use crate::config::DetectedOs;
use crate::error::SshError;

use super::handler::ClientHandler;

/// Execute a command on the remote host and return its stdout output.
async fn exec_command(
    handle: &mut russh::client::Handle<ClientHandler>,
    command: &str,
) -> Result<String, SshError> {
    let timeout_result = timeout(Duration::from_secs(5), async {
        let mut channel = handle
            .channel_open_session()
            .await
            .map_err(|e| SshError::Channel(format!("Failed to open channel: {}", e)))?;

        channel
            .exec(true, command)
            .await
            .map_err(|e| SshError::Channel(format!("Failed to exec '{}': {}", command, e)))?;

        let mut output = String::new();

        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    if let Ok(s) = std::str::from_utf8(&data) {
                        output.push_str(s);
                    }
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    tracing::debug!("{} stderr ({} bytes)", command, data.len());
                }
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                    break;
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    if exit_status != 0 {
                        tracing::debug!("{} exited with status {}", command, exit_status);
                    }
                }
                Some(_) => {}
            }
        }

        Ok(output)
    })
    .await;

    match timeout_result {
        Ok(result) => result,
        Err(_) => Err(SshError::Channel(format!(
            "Command '{}' timed out",
            command
        ))),
    }
}

/// Detect the operating system of a remote host using an existing SSH connection.
///
/// This function:
/// 1. Runs `uname -s` to detect the OS family (Linux, Darwin, FreeBSD, etc.)
/// 2. For Linux systems, also reads `/etc/os-release` to identify the specific distribution
pub async fn detect_os(
    handle: &mut russh::client::Handle<ClientHandler>,
) -> Result<DetectedOs, SshError> {
    // First, detect OS family with uname -s
    let uname_output = exec_command(handle, "uname -s").await?;
    let mut os = DetectedOs::from_uname(&uname_output);
    tracing::info!("Detected OS family: {:?}", os);

    // For Linux, try to identify the specific distribution
    if os.is_linux() {
        tracing::debug!("Linux detected, attempting to read /etc/os-release");
        match exec_command(handle, "cat /etc/os-release 2>/dev/null").await {
            Ok(os_release) => {
                tracing::debug!("os-release content ({} bytes)", os_release.len());
                if !os_release.is_empty() {
                    if let Some(distro) = DetectedOs::from_os_release(&os_release) {
                        tracing::info!("Detected Linux distro: {:?}", distro);
                        os = distro;
                    } else {
                        tracing::warn!("Could not parse distro from os-release");
                    }
                } else {
                    tracing::warn!("/etc/os-release was empty");
                }
            }
            Err(e) => {
                tracing::warn!("Could not read /etc/os-release: {}", e);
            }
        }
    } else {
        tracing::debug!("OS is not Linux ({:?}), skipping distro detection", os);
    }

    tracing::info!("Final detected OS: {:?}", os);
    Ok(os)
}
