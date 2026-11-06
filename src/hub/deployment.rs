//! Versioned deployment protocol and local-only recovery records.
use crate::{
    config::Host,
    ssh::{SshEvent, management::ManagementConnection},
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, watch},
};
use uuid::Uuid;

pub const DEFAULT_IMAGE: &str = "ghcr.io/digitalpals/portal-hub:0.5.0-beta.17";
const HELPER: &str = include_str!("../../deploy/hub/portal_hub_deploy.py");

#[cfg(test)]
#[path = "deployment_transport_tests.rs"]
mod transport_tests;

/// The privileged interpreter reads authenticated source directly from SSH stdin.
/// It never opens a script or imports modules from the SSH user's writable path.
fn helper_command(source: &str) -> String {
    format!(
        "/usr/bin/python3 -I -c {}",
        crate::ssh::management::shell_quote(&helper_loader(source))
    )
}

fn helper_loader(source: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(source.as_bytes()));
    format!(
        "import sys,hashlib; source=sys.stdin.buffer.read({}); \
         expected='{}'; \
         valid=len(source)=={} and hashlib.sha256(source).hexdigest()==expected; \
         valid or sys.exit('Installer integrity check failed'); \
         sys.argv=['portal_hub_deploy.py','--request-stdin']; \
         exec(compile(source,'<portal-hub-deploy>','exec'),{{'__name__':'__main__','__file__':'<portal-hub-deploy>'}})",
        source.len(),
        digest,
        source.len()
    )
}

fn helper_input(source: &str, request: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(source.len() + request.len());
    input.extend_from_slice(source.as_bytes());
    input.extend_from_slice(request);
    input
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentAction {
    Inspect,
    Install,
    Status,
    ConfigureHttps,
    Claim,
    Doctor,
    Logs,
    Update,
    Backup,
    Uninstall,
}
impl DeploymentAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Inspect => "Check server",
            Self::Install => "Install Hub",
            Self::Status => "Refresh status",
            Self::ConfigureHttps => "Enable private HTTPS",
            Self::Claim => "Create owner",
            Self::Doctor => "Check readiness",
            Self::Logs => "View diagnostics",
            Self::Update => "Update Hub",
            Self::Backup => "Back up Hub",
            Self::Uninstall => "Uninstall Hub",
        }
    }
    pub fn interrupts(self) -> bool {
        matches!(
            self,
            Self::Install
                | Self::ConfigureHttps
                | Self::Claim
                | Self::Update
                | Self::Backup
                | Self::Uninstall
        )
    }
}

/// Password has a redacted Debug implementation and is serialized only by
/// stdin_json; neither persistence nor tracing ever sees this request.
#[derive(Debug, Clone)]
pub struct DeploymentRequest {
    pub action: DeploymentAction,
    pub deployment_id: Uuid,
    pub hostname: String,
    pub hub_image: String,
    pub image_archive: Option<String>,
    pub image_archive_sha256: Option<String>,
    pub install_docker: bool,
    pub allow_session_interruptions: bool,
    pub username: String,
    pub password: SecretString,
}
impl DeploymentRequest {
    pub fn stdin_json(&self) -> Result<Vec<u8>, String> {
        if matches!(
            self.action,
            DeploymentAction::Install | DeploymentAction::Update
        ) {
            validate_image(&self.hub_image)?;
        }
        let mut value = serde_json::json!({"protocol_version":1,"action":self.action,"deployment_id":self.deployment_id});
        let object = value.as_object_mut().unwrap();
        if matches!(
            self.action,
            DeploymentAction::Install | DeploymentAction::Update
        ) {
            object.insert("hub_image".into(), self.hub_image.clone().into());
            if let Some(archive) = &self.image_archive {
                object.insert("image_archive".into(), archive.clone().into());
                let digest = self
                    .image_archive_sha256
                    .as_ref()
                    .filter(|digest| {
                        digest.len() == 64
                            && digest
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    })
                    .ok_or("The uploaded image requires its SHA-256 checksum")?;
                object.insert("image_archive_sha256".into(), digest.clone().into());
            }
        }
        if self.action == DeploymentAction::Install {
            object.insert("hostname".into(), self.hostname.clone().into());
            object.insert("install_docker".into(), self.install_docker.into());
        }
        if self.action.interrupts() {
            object.insert(
                "allow_session_interruptions".into(),
                self.allow_session_interruptions.into(),
            );
        }
        if self.action == DeploymentAction::Uninstall {
            object.insert("delete_data".into(), false.into());
        }
        if self.action == DeploymentAction::Claim {
            if !(2..=64).contains(&self.username.len())
                || !(12..=256).contains(&self.password.expose_secret().len())
            {
                return Err(
                    "Use an owner name of 2–64 characters and a password of 12–256 bytes".into(),
                );
            }
            object.insert("username".into(), self.username.clone().into());
            object.insert("password".into(), self.password.expose_secret().into());
        }
        serde_json::to_vec(&value).map_err(|_| "Could not prepare deployment request".into())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeploymentResponse {
    pub protocol_version: u16,
    pub ok: bool,
    pub phase: String,
    pub message: String,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub data: serde_json::Value,
}
impl DeploymentResponse {
    pub fn parse(raw: &str, secret: &SecretString) -> Result<Self, String> {
        if raw.len() > 128 * 1024 {
            return Err("Server response exceeded the diagnostic limit".into());
        }
        let redacted = if secret.expose_secret().is_empty() {
            raw.to_string()
        } else {
            raw.replace(secret.expose_secret(), "[redacted]")
        };
        let mut response: Self = serde_json::from_str(&redacted).map_err(|_| "The server returned an invalid deployment response. Check Python 3 and the bundled helper version.".to_string())?;
        if response.protocol_version != 1 {
            return Err(
                "This deployment helper protocol is not supported. Update Portal before retrying."
                    .into(),
            );
        }
        // Redact after parsing too (JSON can escape characters in a password).
        if !secret.expose_secret().is_empty() {
            response.message = response
                .message
                .replace(secret.expose_secret(), "[redacted]");
            response.phase = response.phase.replace(secret.expose_secret(), "[redacted]");
            response.error_code = response
                .error_code
                .map(|value| value.replace(secret.expose_secret(), "[redacted]"));
            redact_value(&mut response.data, secret.expose_secret());
        }
        response.message = bounded_text(&response.message, 2048);
        for field in ["auth_url", "approval_url"] {
            if let Some(url) = response.data.get(field).and_then(|v| v.as_str())
                && !url.is_empty()
            {
                validate_enrollment_url(url)?;
            }
        }
        if let Some(url) = response.data.get("public_url").and_then(|v| v.as_str())
            && !url.is_empty()
        {
            validate_public_url(url)?;
        }
        Ok(response)
    }
    pub fn bool(&self, key: &str) -> bool {
        self.data
            .get(key)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
    pub fn string(&self, key: &str) -> Option<&str> {
        self.data
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
    }
    pub fn sessions(&self) -> Option<u64> {
        self.data.get("active_sessions").and_then(|v| v.as_u64())
    }
    pub fn guidance(&self) -> &'static str {
        match self.phase.as_str() {
            "awaiting_login" => "Connect Tailscale in your browser, then refresh status.",
            "awaiting_approval" => {
                "This server needs approval from your tailnet administrator. Approve it, then refresh status."
            }
            "awaiting_owner" => "Create the Hub owner before enabling access.",
            "awaiting_https" if !self.bool("owner_exists") => {
                "Create the Hub owner, then enable private HTTPS. If Tailscale requests consent, approve it in your browser, then retry."
            }
            "awaiting_https" => {
                "Enable private HTTPS. If Tailscale requests consent, approve it in your browser, then retry."
            }
            "ready" => {
                "The server is ready. Verify its connection, then sign in and choose your preferences."
            }
            "not_installed" => "Check this server, then install your Hub.",
            _ => "Refresh status to see the next step.",
        }
    }
}
fn redact_value(value: &mut serde_json::Value, secret: &str) {
    match value {
        serde_json::Value::String(text) => *text = text.replace(secret, "[redacted]"),
        serde_json::Value::Array(values) => values.iter_mut().for_each(|v| redact_value(v, secret)),
        serde_json::Value::Object(values) => {
            values.values_mut().for_each(|v| redact_value(v, secret))
        }
        _ => {}
    }
}
pub fn bounded_text(text: &str, length: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(length)
        .collect()
}

pub fn validate_enrollment_url(input: &str) -> Result<url::Url, String> {
    let url = validate_https(input)?;
    if !matches!(
        url.host_str(),
        Some("login.tailscale.com" | "console.tailscale.com")
    ) {
        return Err("The server returned an unexpected enrollment address. Only Tailscale's HTTPS sign-in and approval pages can be opened.".into());
    }
    Ok(url)
}
pub fn validate_public_url(input: &str) -> Result<url::Url, String> {
    let url = validate_https(input)?;
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(
            "The Hub HTTPS address must be an origin without a path, query or fragment".into(),
        );
    }
    Ok(url)
}
fn validate_https(input: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(input)
        .map_err(|_| "The server returned an invalid HTTPS address".to_string())?;
    if input.len() > 2048
        || input
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
        || url.fragment().is_some()
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|p| p != 443)
    {
        return Err(
            "The server address must use HTTPS without credentials or a nonstandard port".into(),
        );
    }
    Ok(url)
}
pub fn validate_image(image: &str) -> Result<(), String> {
    let pinned = if let Some((_, digest)) = image.rsplit_once('@') {
        digest.starts_with("sha256:")
            && digest.len() == 71
            && digest[7..].chars().all(|c| c.is_ascii_hexdigit())
    } else {
        image.rsplit('/').next().and_then(|name| name.rsplit_once(':')).is_some_and(|(_, tag)| {
            static VERSION: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
            VERSION.get_or_init(|| regex::Regex::new(r"^v?(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?(?:\+[0-9A-Za-z.-]+)?$").unwrap()).is_match(tag)
        })
    };
    if !pinned
        || image.chars().any(|c| c.is_whitespace() || c.is_control())
        || image.starts_with('-')
    {
        return Err(
            "Choose a Hub image with a fixed version or SHA-256 digest; latest is not supported"
                .into(),
        );
    }
    Ok(())
}

pub fn validate_hostname(hostname: &str) -> Result<(), String> {
    let valid = (1..=63).contains(&hostname.len())
        && hostname
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && hostname
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && hostname
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric);
    if valid {
        Ok(())
    } else {
        Err("Use a lowercase server name of 1–63 letters, digits or hyphens, starting and ending with a letter or digit (for example, my-hub).".into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentRecord {
    pub deployment_id: Uuid,
    pub ssh_host_id: Uuid,
    pub hostname: String,
    pub hub_image: String,
    #[serde(default)]
    pub public_url: Option<String>,
}
impl std::fmt::Display for DeploymentRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({})",
            self.hostname,
            &self.deployment_id.to_string()[..8]
        )
    }
}
impl PartialEq for DeploymentRecord {
    fn eq(&self, other: &Self) -> bool {
        self.deployment_id == other.deployment_id
    }
}
impl Eq for DeploymentRecord {}
fn records_path() -> Result<PathBuf, String> {
    crate::config::paths::config_dir()
        .map(|p| p.join("hub-deployments.json"))
        .ok_or_else(|| "Could not locate local recovery records".into())
}
fn load_records() -> Result<Vec<DeploymentRecord>, String> {
    let path = records_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes =
        crate::fs_utils::read_regular_file_limited(&path, 1024 * 1024, "Hub recovery records")?;
    serde_json::from_slice(&bytes).map_err(|_| "Could not read Hub recovery records. Restore hub-deployments.json before creating another deployment.".into())
}
pub fn save_record(record: &DeploymentRecord) -> Result<(), String> {
    let mut records = load_records()?;
    if let Some(old) = records
        .iter_mut()
        .find(|r| r.deployment_id == record.deployment_id)
    {
        *old = record.clone();
    } else {
        records.push(record.clone());
    }
    crate::config::paths::ensure_config_dir()
        .map_err(|_| "Could not create local recovery directory")?;
    crate::config::write_atomic(
        &records_path()?,
        &serde_json::to_string_pretty(&records).map_err(|_| "Could not encode recovery records")?,
    )
    .map_err(|_| {
        "Could not save the Hub recovery record. Check local disk space and permissions.".into()
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SetupChoice {
    #[default]
    Existing,
    Server,
}
#[derive(Debug)]
pub struct ManagedHubState {
    pub choice: SetupChoice,
    pub records: Vec<DeploymentRecord>,
    pub selected: Option<DeploymentRecord>,
    pub host_id: Option<Uuid>,
    pub hostname: String,
    pub image: String,
    pub archive: Option<PathBuf>,
    pub install_docker: bool,
    pub allow_interruptions: bool,
    pub uninstall_confirmed: bool,
    pub owner_name: String,
    pub owner_password: SecretString,
    pub busy: bool,
    pub operation: Uuid,
    pub cancel_tx: Option<watch::Sender<bool>>,
    pub progress: String,
    pub error: Option<String>,
    pub response: Option<DeploymentResponse>,
    pub preflight: Option<DeploymentResponse>,
    pub last_action: Option<DeploymentAction>,
    pub diagnostics: Option<String>,
}
impl Default for ManagedHubState {
    fn default() -> Self {
        Self {
            choice: SetupChoice::Existing,
            records: Vec::new(),
            selected: None,
            host_id: None,
            hostname: "portal-hub".into(),
            image: DEFAULT_IMAGE.into(),
            archive: None,
            install_docker: false,
            allow_interruptions: false,
            uninstall_confirmed: false,
            owner_name: String::new(),
            owner_password: SecretString::default(),
            busy: false,
            operation: Uuid::nil(),
            cancel_tx: None,
            progress: String::new(),
            error: None,
            response: None,
            preflight: None,
            last_action: None,
            diagnostics: None,
        }
    }
}
impl ManagedHubState {
    pub fn load() -> Self {
        match load_records() {
            Ok(records) => Self {
                records,
                ..Self::default()
            },
            Err(error) => Self {
                error: Some(error),
                ..Self::default()
            },
        }
    }
    pub fn cancel(&mut self) {
        if let Some(sender) = self.cancel_tx.take() {
            let _ = sender.send(true);
        }
        self.operation = Uuid::new_v4();
        self.busy = false;
        self.owner_password = SecretString::default();
    }
    pub fn accepts(&self, operation: Uuid) -> bool {
        self.busy && self.operation == operation
    }
    pub fn recovery_hosts(&self, hosts: &[Host]) -> HashSet<Uuid> {
        let mut ids = HashSet::new();
        for record in self.records.iter().chain(self.selected.iter()) {
            ids.insert(record.ssh_host_id);
            if let Some(host) = hosts.iter().find(|h| h.id == record.ssh_host_id)
                && let Ok(chain) = crate::ssh::tunnel::resolve_jump_chain(hosts, host)
            {
                ids.extend(chain.iter().map(|h| h.id));
            }
        }
        ids
    }
}

#[derive(Debug, Clone)]
pub enum ManagementMessage {
    Choose(SetupChoice),
    SelectHost(Uuid),
    Resume(DeploymentRecord),
    NewDeployment,
    Hostname(String),
    Image(String),
    ChooseArchive,
    ArchiveChosen(Uuid, Option<PathBuf>),
    ClearArchive,
    InstallDocker(bool),
    AllowInterruptions(bool),
    ConfirmUninstall(bool),
    OwnerName(String),
    OwnerPassword(SecretString),
    Run(DeploymentAction),
    Cancel,
    OpenEnrollment,
    UseEndpoint,
    Progress(Uuid, String),
    /// Cleanup belongs to the task lifetime, independently of the wizard.
    CleanupFailed {
        operation: Uuid,
        host: Uuid,
        directory: Uuid,
    },
    Completed(Uuid, DeploymentAction, Result<DeploymentResponse, String>),
    HostKey(Uuid, crate::message::VerificationRequestWrapper),
    AuthPrompt(Uuid, crate::message::AuthPromptRequestWrapper),
}

#[derive(Debug)]
pub enum ManagementProgress {
    Status(String),
    /// Locally generated directory id, never an arbitrary server error/path.
    CleanupFailed(Uuid),
}

/// Stream the helper into an isolated privileged interpreter. Only an optional
/// image archive uses an upload directory; credentials appear only in stdin.
pub async fn run_remote(
    host: Host,
    chain: Vec<Host>,
    mut request: DeploymentRequest,
    archive: Option<PathBuf>,
    events: mpsc::Sender<SshEvent>,
    mut cancel: watch::Receiver<bool>,
    progress: mpsc::Sender<ManagementProgress>,
) -> Result<DeploymentResponse, String> {
    let connect = ManagementConnection::connect(
        &host,
        &chain,
        crate::app::services::connection::shared_known_hosts_manager(),
        &events,
    );
    let connection = tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(600), connect) => result.map_err(|_| "SSH connection timed out; retry the server check")??,
        _ = cancel.changed() => return Err("Cancelled".into()),
    };
    let directory = Uuid::new_v4();
    let stage = format!("/tmp/portal-hub-{directory}");
    let operation = async {
        let output = connection
            .execute(&format!("umask 077; mkdir -- {stage}"), b"")
            .await?;
        if output.status != 0 {
            return Err("Could not create a private upload directory on the server".into());
        }
        if let Some(path) = archive {
            let _ = progress
                .send(ManagementProgress::Status(
                    "Uploading the local Hub image…".into(),
                ))
                .await;
            let channel = connection.channel().await?;
            channel
                .request_subsystem(true, "sftp")
                .await
                .map_err(|_| "The SSH server must enable SFTP for image upload")?;
            let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
                .await
                .map_err(|_| "Could not start SFTP")?;
            let file = crate::fs_utils::open_read_regular_file(&path, "Hub image archive")
                .map_err(|_| "Could not open the local Hub image archive")?;
            let mut local = tokio::fs::File::from_std(file);
            let remote_path = format!("{stage}/hub-image.tar");
            let mut remote = sftp
                .open_with_flags(
                    &remote_path,
                    russh_sftp::protocol::OpenFlags::WRITE
                        | russh_sftp::protocol::OpenFlags::CREATE
                        | russh_sftp::protocol::OpenFlags::EXCLUDE,
                )
                .await
                .map_err(|_| "Could not create the remote image archive")?;
            let mut digest = Sha256::new();
            let mut buffer = vec![0u8; 256 * 1024];
            loop {
                let count = local
                    .read(&mut buffer)
                    .await
                    .map_err(|_| "Could not read the local Hub image archive")?;
                if count == 0 {
                    break;
                }
                remote
                    .write_all(&buffer[..count])
                    .await
                    .map_err(|_| "Image upload failed. Check free space on the server.")?;
                digest.update(&buffer[..count]);
            }
            remote
                .shutdown()
                .await
                .map_err(|_| "Could not finish image upload")?;
            request.image_archive = Some(remote_path);
            request.image_archive_sha256 = Some(format!("{:x}", digest.finalize()));
            let _ = sftp.close().await;
        }
        let _ = progress
            .send(ManagementProgress::Status(format!(
                "{}…",
                request.action.label()
            )))
            .await;
        let command = helper_command(HELPER);
        let input = helper_input(HELPER, &request.stdin_json()?);
        // Determine actual uid, rather than trusting a configured login name.
        let uid = connection.execute("id -u", b"").await?;
        let output = if uid.status == 0 && uid.stdout.trim() == "0" {
            connection.execute(&command, &input).await?
        } else {
            connection
                .execute_sudo(&command, &input, &host, &events)
                .await?
        };
        if output.stdout.trim().is_empty() {
            return Err("The installer did not return a result. Check Python 3 and sudo permissions, then refresh status before retrying.".into());
        }
        let response = DeploymentResponse::parse(&output.stdout, &request.password)?;
        if let Some(id) = response.string("deployment_id")
            && Uuid::parse_str(id).ok() != Some(request.deployment_id)
        {
            return Err("The server returned a result for another deployment. Refresh status before continuing.".into());
        }
        Ok(response)
    };
    let result = tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(1200), operation) => result.unwrap_or_else(|_| Err("The server operation timed out. It may still finish; refresh status before retrying.".into())),
        _ = cancel.changed() => Err("Stopped waiting. A server operation may still finish; refresh status before retrying.".into()),
    };
    // This path is generated locally and never comes from a server response.
    let cleanup_command = format!("rm -rf -- {stage}");
    let cleanup = connection.execute(&cleanup_command, b"");
    let cleaned = matches!(tokio::time::timeout(Duration::from_secs(10), cleanup).await, Ok(Ok(output)) if output.status == 0);
    let _ = tokio::time::timeout(Duration::from_secs(10), connection.disconnect()).await;
    if !cleaned {
        let _ = progress
            .send(ManagementProgress::CleanupFailed(directory))
            .await;
        return Err(format!(
            "The operation stopped, but its temporary upload directory could not be removed: {stage}. Remove this directory through SSH, then refresh Hub status before retrying."
        ));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    #[test]
    fn https_guidance_accounts_for_owner_creation() {
        let mut response = DeploymentResponse::parse(r#"{"protocol_version":1,"ok":true,"phase":"awaiting_https","message":"Pending","data":{"owner_exists":false}}"#, &SecretString::default()).unwrap();
        assert!(response.guidance().starts_with("Create the Hub owner"));
        response.data["owner_exists"] = true.into();
        assert!(response.guidance().starts_with("Enable private HTTPS"));
        assert!(!response.guidance().contains("Create the Hub owner"));
    }

    #[test]
    fn elevated_helper_uses_authenticated_stdin_and_isolated_imports() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("hashlib.py"),
            "raise RuntimeError('untrusted module imported')",
        )
        .unwrap();
        let source = "import sys,json\nprint(json.dumps(json.loads(sys.stdin.buffer.read())))\n";
        let mut process = Command::new("python3")
            .args(["-I", "-c", &helper_loader(source)])
            .env("PYTHONPATH", directory.path())
            .current_dir(directory.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        process
            .stdin
            .take()
            .unwrap()
            .write_all(&helper_input(source, br#"{"operation":"inspect"}"#))
            .unwrap();
        let output = process.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["operation"],
            "inspect"
        );
        assert!(!helper_command(source).contains(directory.path().to_str().unwrap()));
    }

    #[test]
    fn elevated_helper_rejects_replaced_source_before_execution() {
        let trusted = "print('trusted')\n";
        let replaced = "print('changed')\n";
        let mut process = Command::new("python3")
            .args(["-I", "-c", &helper_loader(trusted)])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        process
            .stdin
            .take()
            .unwrap()
            .write_all(&helper_input(replaced, b"{}"))
            .unwrap();
        let output = process.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("integrity check failed"));
    }
    #[test]
    fn browser_enrollment_rejects_credentials_http_and_untrusted_domains() {
        for url in [
            "http://login.tailscale.com/a",
            "https://login.tailscale.com.evil/a",
            "https://user:secret@login.tailscale.com/a",
            "javascript:alert(1)",
            "https://login.tailscale.com:8443/a",
            "https://evil.test/?next=login.tailscale.com",
        ] {
            assert!(validate_enrollment_url(url).is_err(), "{url}");
        }
        assert!(validate_enrollment_url("https://login.tailscale.com/a/123").is_ok());
        assert!(validate_enrollment_url("https://console.tailscale.com/admin/machines").is_ok());
    }
    #[test]
    fn response_parser_requires_version_and_redacts_escaped_secrets() {
        let secret = SecretString::from("secret\"password");
        let raw = serde_json::json!({"protocol_version":1,"ok":false,"phase":"error","message":"secret\"password", "data":{"logs":"secret\"password"}}).to_string();
        let parsed = DeploymentResponse::parse(&raw, &secret).unwrap();
        assert!(!format!("{parsed:?}").contains("password"));
        for raw in [
            "{}",
            "not json",
            "{\"protocol_version\":2,\"ok\":true,\"phase\":\"ready\",\"message\":\"ok\"}",
        ] {
            assert!(DeploymentResponse::parse(raw, &secret).is_err());
        }
    }
    #[test]
    fn request_secrets_only_enter_claim_stdin_and_uninstall_retains_data() {
        let mut request = DeploymentRequest {
            action: DeploymentAction::Claim,
            deployment_id: Uuid::new_v4(),
            hostname: "hub".into(),
            hub_image: DEFAULT_IMAGE.into(),
            image_archive: None,
            image_archive_sha256: None,
            install_docker: false,
            allow_session_interruptions: false,
            username: "owner".into(),
            password: SecretString::from("very-secret-password"),
        };
        assert!(!format!("{request:?}").contains("very-secret-password"));
        let value: serde_json::Value =
            serde_json::from_slice(&request.stdin_json().unwrap()).unwrap();
        assert_eq!(value["password"], "very-secret-password");
        assert_eq!(value["protocol_version"], 1);
        request.action = DeploymentAction::Uninstall;
        let value: serde_json::Value =
            serde_json::from_slice(&request.stdin_json().unwrap()).unwrap();
        assert!(value.get("password").is_none());
        assert_eq!(value["delete_data"], false);
    }
    #[test]
    fn cancelled_management_results_cannot_complete_a_new_operation() {
        let mut state = ManagedHubState {
            operation: Uuid::new_v4(),
            busy: true,
            ..Default::default()
        };
        let operation = state.operation;
        assert!(state.accepts(operation));
        state.cancel();
        assert!(!state.accepts(operation));
        state.busy = true;
        assert!(!state.accepts(operation));
    }
    #[test]
    fn images_must_be_pinned() {
        assert!(validate_image(DEFAULT_IMAGE).is_ok());
        for image in [
            "portal-hub",
            "portal-hub:latest",
            "registry:5000/portal-hub",
            "portal-hub@sha256:bad",
        ] {
            assert!(validate_image(image).is_err());
        }
    }
}
