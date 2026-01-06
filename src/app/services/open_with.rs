use iced::Task;
use uuid::Uuid;

use crate::message::{Message, SftpMessage};
use crate::sftp::SharedSftpSession;

pub fn temp_open_dir(id: Uuid) -> std::path::PathBuf {
    std::env::temp_dir().join("portal_open").join(format!("{}", id))
}

pub fn temp_open_path(id: Uuid, file_name: &str) -> std::path::PathBuf {
    temp_open_dir(id).join(file_name)
}

pub fn open_local_default(path: std::path::PathBuf) -> Task<Message> {
    Task::perform(
        async move { open::that(&path).map_err(|e| format!("Failed to open file: {}", e)) },
        |result| Message::Sftp(SftpMessage::OpenWithResult(result)),
    )
}

pub fn open_remote_default(sftp: SharedSftpSession, remote_path: std::path::PathBuf) -> Task<Message> {
    Task::perform(
        async move {
            let file_name = remote_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let temp_id = Uuid::new_v4();
            let temp_dir = temp_open_dir(temp_id);
            let local_path = temp_open_path(temp_id, &file_name);

            tokio::fs::create_dir_all(&temp_dir)
                .await
                .map_err(|e| format!("Failed to create temp directory: {}", e))?;
            sftp.download(&remote_path, &local_path)
                .await
                .map_err(|e| format!("Failed to download file: {}", e))?;
            open::that(&local_path).map_err(|e| format!("Failed to open file: {}", e))?;
            Ok(())
        },
        |result| Message::Sftp(SftpMessage::OpenWithResult(result)),
    )
}

pub fn open_local_with_command(path: std::path::PathBuf, command: String) -> Task<Message> {
    Task::perform(
        async move {
            let status = std::process::Command::new(&command).arg(&path).spawn();
            match status {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("Failed to run '{}': {}", command, e)),
            }
        },
        |result| Message::Sftp(SftpMessage::OpenWithResult(result)),
    )
}

pub fn open_remote_with_command(
    sftp: SharedSftpSession,
    remote_path: std::path::PathBuf,
    command: String,
) -> Task<Message> {
    Task::perform(
        async move {
            let file_name = remote_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let temp_id = Uuid::new_v4();
            let temp_dir = temp_open_dir(temp_id);
            let local_path = temp_open_path(temp_id, &file_name);

            tokio::fs::create_dir_all(&temp_dir)
                .await
                .map_err(|e| format!("Failed to create temp directory: {}", e))?;
            sftp.download(&remote_path, &local_path)
                .await
                .map_err(|e| format!("Failed to download file: {}", e))?;

            let status = std::process::Command::new(&command).arg(&local_path).spawn();
            match status {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("Failed to run '{}': {}", command, e)),
            }
        },
        |result| Message::Sftp(SftpMessage::OpenWithResult(result)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_open_path_is_stable_for_id() {
        let id = Uuid::new_v4();
        let path = temp_open_path(id, "notes.txt");
        let expected = std::env::temp_dir()
            .join("portal_open")
            .join(format!("{}", id))
            .join("notes.txt");
        assert_eq!(path, expected);
    }
}
