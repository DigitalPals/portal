//! Versioned deployment protocol and local-only recovery records.
use crate::{
    config::Host,
    ssh::{SshEvent, management::ManagementConnection},
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, watch},
};
use uuid::Uuid;

/// Portal and Hub share one version: each Portal release deploys its own Hub.
pub const DEFAULT_IMAGE: &str =
    concat!("ghcr.io/digitalpals/portal-hub:", env!("CARGO_PKG_VERSION"));
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

/// Exit status of [`bootstrap_command`] when no usable Python could be installed.
const PYTHON_UNAVAILABLE: u32 = 86;

/// Package managers run with stdin closed, so the authenticated helper source
/// queued on SSH stdin is read only by the verified loader afterwards.
const PYTHON_BOOTSTRAP: &str = r#"PATH=/usr/sbin:/usr/bin:/sbin:/bin; export PATH
usable() { /usr/bin/python3 -c 'import sys; sys.exit(sys.version_info < (3, 9))' </dev/null >/dev/null 2>&1; }
if ! usable; then
  if command -v apt-get >/dev/null 2>&1; then
    DEBIAN_FRONTEND=noninteractive apt-get update -q </dev/null >&2
    DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends python3 </dev/null >&2
  elif command -v dnf >/dev/null 2>&1; then
    dnf install -y python3 </dev/null >&2
  fi
  usable || exit 86
fi"#;

/// The helper command, first installing Python 3 from the server's own
/// package sources when it is missing. Used only with setup consent.
fn bootstrap_command(source: &str) -> String {
    format!(
        "/bin/sh -c {}",
        crate::ssh::management::shell_quote(&format!(
            "{PYTHON_BOOTSTRAP}\nexec {}",
            helper_command(source)
        ))
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
    OwnerSetup,
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
            Self::OwnerSetup => "Set up owner passkey",
            Self::Doctor => "Check readiness",
            Self::Logs => "View diagnostics",
            Self::Update => "Update Hub",
            Self::Backup => "Back up Hub",
            Self::Uninstall => "Uninstall Hub",
        }
    }
    /// Actions that may replace containers. Owner setup only does so when the
    /// Hub's address changed, and the helper still refuses that with sessions.
    pub fn interrupts(self) -> bool {
        matches!(
            self,
            Self::Install | Self::ConfigureHttps | Self::Update | Self::Backup | Self::Uninstall
        )
    }
}

/// What an owner passkey setup link allows on the Hub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerSetupMode {
    /// The first passkey; creates the owner on a new Hub.
    #[default]
    Setup,
    /// Another passkey next to the existing ones.
    Add,
    /// Replace every passkey and sign out every device (recovery).
    Reset,
}

/// Requests carry no secrets: the owner signs in with a passkey created in
/// the browser, never with a credential Portal sends to the server.
#[derive(Debug, Clone)]
pub struct DeploymentRequest {
    pub action: DeploymentAction,
    pub deployment_id: Uuid,
    pub hostname: String,
    pub hub_image: String,
    pub image_archive: Option<String>,
    pub image_archive_sha256: Option<String>,
    pub allow_session_interruptions: bool,
    pub username: String,
    pub owner_mode: OwnerSetupMode,
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
            // Installation adds Docker when it is missing; starting setup is the consent.
            object.insert("install_docker".into(), true.into());
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
        if self.action == DeploymentAction::OwnerSetup {
            object.insert(
                "mode".into(),
                serde_json::to_value(self.owner_mode).unwrap_or_default(),
            );
            if self.owner_mode == OwnerSetupMode::Setup {
                validate_owner_username(&self.username)?;
                object.insert("username".into(), self.username.clone().into());
            }
        }
        serde_json::to_vec(&value).map_err(|_| "Could not prepare deployment request".into())
    }
}

/// A one-time link that lets the person create the owner passkey in their
/// browser. The token never enters diagnostics, logs or saved state.
#[derive(Clone)]
pub struct OwnerSetupLink {
    pub token: SecretString,
    pub username: String,
}
impl std::fmt::Debug for OwnerSetupLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerSetupLink")
            .field("username", &self.username)
            .finish_non_exhaustive()
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
    /// Moved out of `data` while parsing, so diagnostics never show it.
    #[serde(skip)]
    pub owner_setup: Option<OwnerSetupLink>,
}
impl DeploymentResponse {
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.len() > 128 * 1024 {
            return Err("Server response exceeded the diagnostic limit".into());
        }
        let mut response: Self = serde_json::from_str(raw).map_err(|_| "The server returned an invalid deployment response. Check Python 3 and the bundled helper version.".to_string())?;
        if response.protocol_version != 1 {
            return Err(
                "This deployment helper protocol is not supported. Update Portal before retrying."
                    .into(),
            );
        }
        if let Some(setup) = response
            .data
            .as_object_mut()
            .and_then(|data| data.remove("owner_setup"))
        {
            let token = setup.get("token").and_then(|v| v.as_str()).unwrap_or("");
            if token.len() != 43
                || !token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err("The server returned an invalid passkey setup link".into());
            }
            response.owner_setup = Some(OwnerSetupLink {
                token: token.to_string().into(),
                username: bounded_text(
                    setup.get("username").and_then(|v| v.as_str()).unwrap_or(""),
                    64,
                ),
            });
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

/// Whether a Hub reporting `hub_version` is older than this Portal, which then
/// offers to update it to the Hub image of its own version.
pub fn hub_is_older_than_portal(hub_version: &str) -> bool {
    fn key(version: &str) -> Option<([u64; 3], bool)> {
        let (core, pre) = match version.trim_start_matches('v').split_once('-') {
            Some((core, _)) => (core, true),
            None => (version.trim_start_matches('v'), false),
        };
        let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
        let numbers = [parts.next()??, parts.next()??, parts.next()??];
        // A prerelease sorts before its release.
        Some((numbers, !pre))
    }
    matches!(
        (key(hub_version), key(env!("CARGO_PKG_VERSION"))),
        (Some(hub), Some(portal)) if hub < portal
    )
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
    /// Account name Portal used when it created the owner.
    #[serde(default)]
    pub owner_username: Option<String>,
}

pub fn validate_owner_username(name: &str) -> Result<(), String> {
    if (2..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'@'))
    {
        Ok(())
    } else {
        Err(
            "Use an owner account name of 2–64 letters, digits, dots, dashes, underscores or @."
                .into(),
        )
    }
}

/// The local login name as a Hub account name, for a new owner.
pub fn default_owner_username() -> String {
    owner_username(
        std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .ok(),
    )
}

/// Hub account names allow 2–64 ASCII letters, digits and `._-@`.
fn owner_username(local: Option<String>) -> String {
    let name: String = local
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@'))
        .take(64)
        .collect();
    if name.len() >= 2 {
        name
    } else {
        "owner".into()
    }
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
    /// The one-click setup is running (or finished) rather than a manual action.
    pub automated: bool,
    pub show_advanced: bool,
    /// Last Tailscale page opened for the person, to reopen it on request.
    pub browser_url: Option<String>,
    pub records: Vec<DeploymentRecord>,
    pub selected: Option<DeploymentRecord>,
    pub host_id: Option<Uuid>,
    pub hostname: String,
    pub image: String,
    pub archive: Option<PathBuf>,
    pub allow_interruptions: bool,
    pub uninstall_confirmed: bool,
    /// Resetting the owner passkeys signs out every device: confirm first.
    pub reset_confirmed: bool,
    pub owner_name: String,
    pub busy: bool,
    pub operation: Uuid,
    pub cancel_tx: Option<watch::Sender<bool>>,
    pub progress: String,
    pub error: Option<String>,
    pub response: Option<DeploymentResponse>,
    pub last_action: Option<DeploymentAction>,
    pub diagnostics: Option<String>,
}
impl Default for ManagedHubState {
    fn default() -> Self {
        Self {
            choice: SetupChoice::Existing,
            automated: false,
            show_advanced: false,
            browser_url: None,
            records: Vec::new(),
            selected: None,
            host_id: None,
            hostname: "portal-hub".into(),
            image: DEFAULT_IMAGE.into(),
            archive: None,
            allow_interruptions: false,
            uninstall_confirmed: false,
            reset_confirmed: false,
            owner_name: String::new(),
            busy: false,
            operation: Uuid::nil(),
            cancel_tx: None,
            progress: String::new(),
            error: None,
            response: None,
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
    }
    pub fn accepts(&self, operation: Uuid) -> bool {
        self.busy && self.operation == operation
    }
    /// The private HTTPS address of the selected Hub once it is ready for sign-in.
    pub fn ready_url(&self) -> Option<String> {
        self.response
            .as_ref()
            .filter(|r| r.ok && r.phase == "ready" && r.bool("owner_ready"))
            .and_then(|r| r.string("public_url"))
            .filter(|url| validate_public_url(url).is_ok())
            .map(str::to_string)
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
    AllowInterruptions(bool),
    ConfirmUninstall(bool),
    ConfirmReset(bool),
    OwnerName(String),
    Run(DeploymentAction),
    /// Create a passkey setup link over SSH, then open it in the browser.
    SetUpPasskey(OwnerSetupMode),
    /// Update the Hub to the image of this Portal version.
    UpdateToPortalVersion,
    /// One-click setup: everything from server check to a signed-in Hub.
    Deploy,
    ToggleAdvanced,
    /// The automated run reached a Tailscale page the person must complete.
    OpenBrowser(Uuid, String),
    ReopenBrowser,
    AutoCompleted(Uuid, Result<DeploymentResponse, String>),
    Cancel,
    /// Connect this device to the selected ready Hub and sign in.
    ConnectDevice,
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
    /// A validated Tailscale login, approval or HTTPS consent page.
    OpenBrowser(String),
    /// Locally generated directory id, never an arbitrary server error/path.
    CleanupFailed(Uuid),
}

/// One verified SSH connection for a sequence of helper calls. The sudo
/// password, when needed, is kept in memory for this session only.
struct DeploymentSession {
    connection: ManagementConnection,
    host: Host,
    events: mpsc::Sender<SshEvent>,
    root: bool,
    sudo: Option<SecretString>,
    install_python: bool,
    stage: String,
    staged: bool,
}

impl DeploymentSession {
    async fn open(
        host: Host,
        chain: &[Host],
        events: mpsc::Sender<SshEvent>,
        install_python: bool,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<Self, String> {
        let connect = ManagementConnection::connect(
            &host,
            chain,
            crate::app::services::connection::shared_known_hosts_manager(),
            &events,
        );
        let connection = tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(600), connect) => result.map_err(|_| "SSH connection timed out; retry the server check")??,
            _ = cancel.changed() => return Err("Cancelled".into()),
        };
        // Determine actual uid, rather than trusting a configured login name.
        let root = match connection.execute("id -u", b"").await {
            Ok(uid) => uid.status == 0 && uid.stdout.trim() == "0",
            Err(error) => {
                connection.disconnect().await;
                return Err(error);
            }
        };
        Ok(Self {
            connection,
            host,
            events,
            root,
            sudo: None,
            install_python,
            stage: format!("/tmp/portal-hub-{}", Uuid::new_v4()),
            staged: false,
        })
    }

    fn directory(&self) -> Uuid {
        Uuid::parse_str(self.stage.trim_start_matches("/tmp/portal-hub-")).unwrap_or_default()
    }

    /// Upload an image archive into a private staging directory, returning
    /// its remote path and the SHA-256 calculated while sending it.
    async fn upload(
        &mut self,
        path: &std::path::Path,
        progress: &mpsc::Sender<ManagementProgress>,
    ) -> Result<(String, String), String> {
        let output = self
            .connection
            .execute(&format!("umask 077; mkdir -- {}", self.stage), b"")
            .await?;
        if output.status != 0 {
            return Err("Could not create a private upload directory on the server".into());
        }
        self.staged = true;
        let _ = progress
            .send(ManagementProgress::Status(
                "Uploading the local Hub image…".into(),
            ))
            .await;
        let channel = self.connection.channel().await?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|_| "The SSH server must enable SFTP for image upload")?;
        let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
            .await
            .map_err(|_| "Could not start SFTP")?;
        let file = crate::fs_utils::open_read_regular_file(path, "Hub image archive")
            .map_err(|_| "Could not open the local Hub image archive")?;
        let mut local = tokio::fs::File::from_std(file);
        let remote_path = format!("{}/hub-image.tar", self.stage);
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
        let _ = sftp.close().await;
        Ok((remote_path, format!("{:x}", digest.finalize())))
    }

    async fn call(&mut self, request: &DeploymentRequest) -> Result<DeploymentResponse, String> {
        let command = if self.install_python {
            bootstrap_command(HELPER)
        } else {
            helper_command(HELPER)
        };
        let input = helper_input(HELPER, &request.stdin_json()?);
        let output = if self.root {
            self.connection.execute(&command, &input).await?
        } else {
            self.connection
                .execute_sudo(&command, &input, &self.host, &self.events, &mut self.sudo)
                .await?
        };
        if output.status == PYTHON_UNAVAILABLE && output.stdout.trim().is_empty() {
            return Err("Python 3.9 or newer is missing on the server and could not be installed automatically. Install python3, then retry.".into());
        }
        if output.stdout.trim().is_empty() {
            return Err("The installer did not return a result. Check Python 3 and sudo permissions, then refresh status before retrying.".into());
        }
        let response = DeploymentResponse::parse(&output.stdout)?;
        if let Some(id) = response.string("deployment_id")
            && Uuid::parse_str(id).ok() != Some(request.deployment_id)
        {
            return Err("The server returned a result for another deployment. Refresh status before continuing.".into());
        }
        Ok(response)
    }

    /// Remove the staging directory (if any) and disconnect. A failed cleanup
    /// is reported separately, as the server keeps the temporary directory.
    async fn close(self, progress: &mpsc::Sender<ManagementProgress>) -> Result<(), String> {
        let mut cleaned = true;
        if self.staged {
            // This path is generated locally and never comes from a server response.
            let command = format!("rm -rf -- {}", self.stage);
            let cleanup = self.connection.execute(&command, b"");
            cleaned = matches!(tokio::time::timeout(Duration::from_secs(10), cleanup).await, Ok(Ok(output)) if output.status == 0);
        }
        let _ = tokio::time::timeout(Duration::from_secs(10), self.connection.disconnect()).await;
        if cleaned {
            return Ok(());
        }
        let _ = progress
            .send(ManagementProgress::CleanupFailed(self.directory()))
            .await;
        Err(format!(
            "The operation stopped, but its temporary upload directory could not be removed: {}. Remove this directory through SSH, then refresh Hub status before retrying.",
            self.stage
        ))
    }
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
    // Only installation may add Python; inspection and status stay read-only.
    let install_python = request.action == DeploymentAction::Install;
    let mut session =
        DeploymentSession::open(host, &chain, events, install_python, &mut cancel).await?;
    let operation = async {
        if let Some(path) = archive {
            let (remote, digest) = session.upload(&path, &progress).await?;
            request.image_archive = Some(remote);
            request.image_archive_sha256 = Some(digest);
        }
        let _ = progress
            .send(ManagementProgress::Status(format!(
                "{}…",
                request.action.label()
            )))
            .await;
        session.call(&request).await
    };
    let result = tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(1200), operation) => result.unwrap_or_else(|_| Err("The server operation timed out. It may still finish; refresh status before retrying.".into())),
        _ = cancel.changed() => Err("Stopped waiting. A server operation may still finish; refresh status before retrying.".into()),
    };
    session.close(&progress).await?;
    result
}

/// Everything the automated setup needs. The owner is created later, by a
/// passkey in the browser, so the plan carries only its account name.
#[derive(Debug, Clone)]
pub struct AutomatedPlan {
    pub deployment_id: Uuid,
    pub hostname: String,
    pub hub_image: String,
    pub archive: Option<PathBuf>,
    pub owner_username: String,
}

/// What the automated run does after a helper response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoStep {
    Ready,
    /// Wait for the person to finish a Tailscale page, opened once per URL.
    Browser {
        url: String,
        then: DeploymentAction,
    },
    Wait,
    Run(DeploymentAction),
    Fail(String),
}

/// Pure decision function, so the sequence is unit-testable without SSH.
pub fn next_step(response: &DeploymentResponse, last: DeploymentAction) -> AutoStep {
    if !response.ok {
        return match response.error_code.as_deref() {
            Some("deployment_busy" | "tailscale_not_ready" | "https_not_ready") => AutoStep::Wait,
            Some(code) => AutoStep::Fail(format!("{} ({code})", response.message)),
            None => AutoStep::Fail(response.message.clone()),
        };
    }
    match response.phase.as_str() {
        "ready" => AutoStep::Ready,
        "awaiting_login" | "awaiting_approval" => {
            match response
                .string("auth_url")
                .or_else(|| response.string("approval_url"))
            {
                Some(url) => AutoStep::Browser {
                    url: url.into(),
                    then: DeploymentAction::Status,
                },
                // Tailscale is still starting and has no login URL yet.
                None => AutoStep::Wait,
            }
        }
        // The setup link is the hand-off: the passkey is created in the browser.
        "awaiting_owner" if response.owner_setup.is_some() => AutoStep::Ready,
        "awaiting_owner" => AutoStep::Run(DeploymentAction::OwnerSetup),
        "awaiting_https" => match response.string("auth_url") {
            // HTTPS certificates need tailnet consent: only an actual
            // certificate attempt proves that, not a status inference.
            Some(url) if last == DeploymentAction::ConfigureHttps => AutoStep::Browser {
                url: url.into(),
                then: DeploymentAction::ConfigureHttps,
            },
            _ => AutoStep::Run(DeploymentAction::ConfigureHttps),
        },
        "not_installed" => AutoStep::Run(DeploymentAction::Install),
        _ => AutoStep::Fail(response.message.clone()),
    }
}

fn step_message(action: DeploymentAction, docker_missing: bool) -> &'static str {
    match action {
        DeploymentAction::Inspect => "Checking the server…",
        DeploymentAction::Install if docker_missing => {
            "Installing Docker and Portal Hub. This can take a few minutes…"
        }
        DeploymentAction::Install => "Installing Portal Hub…",
        DeploymentAction::OwnerSetup => "Preparing your owner passkey…",
        DeploymentAction::ConfigureHttps => "Enabling private HTTPS…",
        _ => "Checking Hub status…",
    }
}

fn browser_message(response: &DeploymentResponse) -> &'static str {
    match response.phase.as_str() {
        "awaiting_approval" => {
            "Approve this Hub in the Tailscale admin console that opened in your browser. Setup continues automatically."
        }
        "awaiting_https" => {
            "Enable HTTPS certificates for your tailnet on the Tailscale page that opened in your browser. Setup continues automatically."
        }
        _ => {
            "Sign in to Tailscale in your browser to connect this Hub. Setup continues automatically."
        }
    }
}

/// Inspect, install (with Docker), wait for Tailscale enrollment and enable
/// HTTPS on one connection. Ends when the Hub is ready, or with a one-time
/// link for creating the owner passkey in the browser.
pub async fn run_automated(
    host: Host,
    chain: Vec<Host>,
    plan: AutomatedPlan,
    events: mpsc::Sender<SshEvent>,
    mut cancel: watch::Receiver<bool>,
    progress: mpsc::Sender<ManagementProgress>,
) -> Result<DeploymentResponse, String> {
    let mut session = DeploymentSession::open(host, &chain, events, true, &mut cancel).await?;
    let mut cancel_wait = cancel.clone();
    let operation = async {
        let request = |action| DeploymentRequest {
            action,
            deployment_id: plan.deployment_id,
            hostname: plan.hostname.clone(),
            hub_image: plan.hub_image.clone(),
            image_archive: None,
            image_archive_sha256: None,
            allow_session_interruptions: false,
            username: plan.owner_username.clone(),
            owner_mode: OwnerSetupMode::Setup,
        };
        let status = |text: &str| {
            let progress = progress.clone();
            let text = text.to_string();
            async move {
                let _ = progress.send(ManagementProgress::Status(text)).await;
            }
        };
        status(step_message(DeploymentAction::Inspect, false)).await;
        let inspect = session.call(&request(DeploymentAction::Inspect)).await?;
        if !inspect.ok {
            return Ok(inspect);
        }
        let blockers = inspection_blockers(&inspect);
        if !blockers.is_empty() {
            return Err(blockers.join(" "));
        }
        let docker_missing =
            !inspect.bool("docker_available") || !inspect.bool("compose_available");
        let mut install = request(DeploymentAction::Install);
        if !inspect.bool("installed")
            && let Some(path) = &plan.archive
        {
            let (remote, digest) = session.upload(path, &progress).await?;
            install.image_archive = Some(remote);
            install.image_archive_sha256 = Some(digest);
        }
        status(step_message(DeploymentAction::Install, docker_missing)).await;
        let mut response = session.call(&install).await?;
        let mut last = DeploymentAction::Install;
        let mut opened: Option<String> = None;
        let mut https_attempts = 0;
        loop {
            let action = match next_step(&response, last) {
                AutoStep::Ready => return Ok(response),
                AutoStep::Fail(message) => return Err(message),
                AutoStep::Run(DeploymentAction::Install) if last == DeploymentAction::Install => {
                    return Err("The server reported that Hub is not installed after installation. Check diagnostics, then retry.".into());
                }
                AutoStep::Run(action) => action,
                AutoStep::Wait => {
                    wait(&mut cancel_wait, POLL_INTERVAL).await?;
                    DeploymentAction::Status
                }
                AutoStep::Browser { url, then } => {
                    if opened.as_deref() != Some(url.as_str()) {
                        validate_enrollment_url(&url)?;
                        let _ = progress
                            .send(ManagementProgress::OpenBrowser(url.clone()))
                            .await;
                        opened = Some(url);
                    }
                    status(browser_message(&response)).await;
                    wait(&mut cancel_wait, POLL_INTERVAL).await?;
                    then
                }
            };
            if action == DeploymentAction::ConfigureHttps {
                https_attempts += 1;
                // Each attempt is bounded by the helper; give tailnet consent
                // and certificate issuance generous time before giving up.
                if https_attempts > MAX_HTTPS_ATTEMPTS {
                    return Err("Private HTTPS did not become ready. Check that HTTPS certificates are enabled for your tailnet, then resume setup.".into());
                }
            }
            if action != DeploymentAction::Status || last != DeploymentAction::Status {
                status(step_message(action, false)).await;
            }
            response = session.call(&request(action)).await?;
            last = action;
        }
    };
    let result = tokio::select! {
        result = tokio::time::timeout(AUTOMATED_DEADLINE, operation) => result.unwrap_or_else(|_| Err("Setup is taking longer than expected. Finish any open Tailscale page, then resume setup.".into())),
        _ = cancel.changed() => Err("Stopped waiting. Changes already made on the server are kept; resume setup to continue.".into()),
    };
    session.close(&progress).await?;
    result
}

const POLL_INTERVAL: Duration = Duration::from_secs(3);
const MAX_HTTPS_ATTEMPTS: u32 = 120;
const AUTOMATED_DEADLINE: Duration = Duration::from_secs(60 * 60);

async fn wait(cancel: &mut watch::Receiver<bool>, duration: Duration) -> Result<(), String> {
    tokio::select! {
        _ = tokio::time::sleep(duration) => Ok(()),
        _ = cancel.changed() => Err("Cancelled".into()),
    }
}

/// Prerequisites the automated run cannot fix itself. Docker and Compose are
/// installed by the run; missing TUN selects Tailscale userspace networking.
pub fn inspection_blockers(inspect: &DeploymentResponse) -> Vec<String> {
    let lxc = inspect.string("virtualization") == Some("lxc");
    inspect
        .data
        .get("blockers")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .filter_map(|blocker| match blocker {
            "docker_unavailable" | "compose_unavailable" => None,
            "unsupported_architecture" => Some("Portal Hub needs a 64-bit Intel/AMD (amd64) or ARM (arm64) server.".to_string()),
            "linux_pidfd_required" => Some("Portal Hub needs Linux 5.3 or newer.".to_string()),
            "tun_unavailable" => Some(if lxc {
                "This Hub was installed with kernel networking, but /dev/net/tun is no longer available. Re-enable TUN for this LXC container on its host.".to_string()
            } else {
                "This Hub was installed with kernel networking, but /dev/net/tun is no longer available on the server.".to_string()
            }),
            other => Some(format!("The server is missing a prerequisite ({}).", bounded_text(other, 64))),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    #[test]
    fn hub_and_portal_share_one_version() {
        let hub: toml::Table = include_str!("../../hub/Cargo.toml").parse().unwrap();
        assert_eq!(
            hub["package"]["version"].as_str(),
            Some(env!("CARGO_PKG_VERSION")),
            "update hub/Cargo.toml together with Cargo.toml"
        );
        assert!(HELPER.contains(&format!("DEFAULT_IMAGE = \"{DEFAULT_IMAGE}\"")));
        assert!(
            include_str!("../../deploy/hub/Dockerfile")
                .contains(&format!("ARG HUB_VERSION={}\n", env!("CARGO_PKG_VERSION")))
        );
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

    fn response(phase: &str, data: serde_json::Value) -> DeploymentResponse {
        DeploymentResponse::parse(
            &serde_json::json!({"protocol_version":1,"ok":true,"phase":phase,"message":"m","data":data})
                .to_string(),
        )
        .unwrap()
    }

    #[test]
    fn automated_steps_follow_the_server_phase() {
        use DeploymentAction::*;
        let login = "https://login.tailscale.com/a/123";
        let dns = "https://login.tailscale.com/admin/dns";
        assert_eq!(
            next_step(&response("ready", serde_json::json!({})), Status),
            AutoStep::Ready
        );
        assert_eq!(
            next_step(
                &response("awaiting_login", serde_json::json!({"auth_url": login})),
                Install
            ),
            AutoStep::Browser {
                url: login.into(),
                then: Status
            }
        );
        assert_eq!(
            next_step(
                &response("awaiting_login", serde_json::json!({"auth_url": null})),
                Install
            ),
            AutoStep::Wait
        );
        assert_eq!(
            next_step(
                &response(
                    "awaiting_approval",
                    serde_json::json!({"approval_url": "https://login.tailscale.com/admin/machines"})
                ),
                Status
            ),
            AutoStep::Browser {
                url: "https://login.tailscale.com/admin/machines".into(),
                then: Status
            }
        );
        // Status suggests the DNS page before any certificate attempt; try first.
        let https = response("awaiting_https", serde_json::json!({"auth_url": dns}));
        assert_eq!(next_step(&https, Status), AutoStep::Run(ConfigureHttps));
        assert_eq!(
            next_step(&https, ConfigureHttps),
            AutoStep::Browser {
                url: dns.into(),
                then: ConfigureHttps
            }
        );
        // The owner comes after HTTPS; a setup link ends the SSH part.
        assert_eq!(
            next_step(&response("awaiting_owner", serde_json::json!({})), Status),
            AutoStep::Run(OwnerSetup)
        );
        let link = response(
            "awaiting_owner",
            serde_json::json!({"owner_setup": {"token": "t".repeat(43), "username": "owner"}}),
        );
        assert_eq!(next_step(&link, OwnerSetup), AutoStep::Ready);
        let error = |code: &str| {
            DeploymentResponse::parse(
                &serde_json::json!({"protocol_version":1,"ok":false,"phase":"error","message":"failed","error_code":code,"data":{}}).to_string(),
            )
            .unwrap()
        };
        assert_eq!(next_step(&error("deployment_busy"), Status), AutoStep::Wait);
        assert_eq!(
            next_step(&error("tailscale_not_ready"), OwnerSetup),
            AutoStep::Wait
        );
        assert_eq!(
            next_step(&error("https_not_ready"), OwnerSetup),
            AutoStep::Wait
        );
        assert_eq!(
            next_step(&error("prerequisites_failed"), Install),
            AutoStep::Fail("failed (prerequisites_failed)".into())
        );
    }

    #[test]
    fn only_unfixable_prerequisites_block_automated_setup() {
        let inspect = response(
            "inspected",
            serde_json::json!({"blockers": ["docker_unavailable", "compose_unavailable"]}),
        );
        assert!(inspection_blockers(&inspect).is_empty());
        let inspect = response(
            "inspected",
            serde_json::json!({"virtualization": "lxc", "blockers": ["tun_unavailable", "unsupported_architecture", "\u{1b}[31mnew"]}),
        );
        let blockers = inspection_blockers(&inspect);
        assert_eq!(blockers.len(), 3);
        assert!(blockers[0].contains("LXC"));
        assert!(blockers[1].contains("arm64"));
        assert!(!blockers[2].contains('\u{1b}'));
    }

    #[test]
    fn older_hubs_are_offered_this_portal_version() {
        assert!(hub_is_older_than_portal("0.5.0-beta.17"));
        assert!(hub_is_older_than_portal("0.0.1"));
        assert!(!hub_is_older_than_portal(env!("CARGO_PKG_VERSION")));
        assert!(!hub_is_older_than_portal("999.0.0"));
        assert!(!hub_is_older_than_portal("not a version"));
    }

    #[test]
    fn owner_names_follow_hub_account_rules() {
        assert_eq!(owner_username(Some("john".into())), "john");
        assert_eq!(owner_username(Some("José Smith".into())), "JosSmith");
        assert_eq!(owner_username(Some("x".into())), "owner");
        assert_eq!(owner_username(None), "owner");
        assert_eq!(owner_username(Some("a".repeat(80))).len(), 64);
        assert!(validate_owner_username(&default_owner_username()).is_ok());
        assert!(validate_owner_username("bad name").is_err());
        assert!(validate_owner_username("x").is_err());
    }

    #[test]
    fn python_bootstrap_runs_the_verified_helper_when_python_exists() {
        let source = "import sys,json\nprint(json.dumps({'ok': len(sys.stdin.buffer.read())}))\n";
        let mut process = Command::new("/bin/sh")
            .args(["-c", &bootstrap_command(source)])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        process
            .stdin
            .take()
            .unwrap()
            .write_all(&helper_input(source, b"{\"request\":1}"))
            .unwrap();
        let output = process.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        // The loader consumed the source; only the request remained for the helper.
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["ok"],
            13
        );
        assert!(PYTHON_BOOTSTRAP.contains("</dev/null"));
        assert!(!PYTHON_BOOTSTRAP.contains("curl"));
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
    fn setup_tokens_leave_the_response_data_and_stay_out_of_debug_output() {
        let token = "A1b2-C3d4_".repeat(4) + "xyz";
        let raw = serde_json::json!({"protocol_version":1,"ok":true,"phase":"awaiting_owner","message":"m",
            "data":{"public_url":"https://hub.example.ts.net","owner_setup":{"token":token,"username":"owner","mode":"setup"}}}).to_string();
        let parsed = DeploymentResponse::parse(&raw).unwrap();
        let link = parsed.owner_setup.as_ref().unwrap();
        assert_eq!(link.token.expose_secret(), token);
        assert_eq!(link.username, "owner");
        assert!(parsed.data.get("owner_setup").is_none());
        assert!(!format!("{parsed:?}").contains(&token));
        assert!(
            !serde_json::to_string(&parsed.data)
                .unwrap()
                .contains(&token)
        );
        for bad in [
            "short",
            "has spaces in it and is long enough to be 43",
            "../../../../../../../../../../../../../../..",
        ] {
            let raw = serde_json::json!({"protocol_version":1,"ok":true,"phase":"awaiting_owner","message":"m",
                "data":{"owner_setup":{"token":bad}}}).to_string();
            assert!(DeploymentResponse::parse(&raw).is_err(), "{bad}");
        }
        for raw in [
            "{}",
            "not json",
            "{\"protocol_version\":2,\"ok\":true,\"phase\":\"ready\",\"message\":\"ok\"}",
        ] {
            assert!(DeploymentResponse::parse(raw).is_err());
        }
    }
    #[test]
    fn only_installation_asks_the_server_to_add_docker() {
        let mut request = DeploymentRequest {
            action: DeploymentAction::Install,
            deployment_id: Uuid::new_v4(),
            hostname: "hub".into(),
            hub_image: DEFAULT_IMAGE.into(),
            image_archive: None,
            image_archive_sha256: None,
            allow_session_interruptions: false,
            username: "owner".into(),
            owner_mode: OwnerSetupMode::Setup,
        };
        let value: serde_json::Value =
            serde_json::from_slice(&request.stdin_json().unwrap()).unwrap();
        assert_eq!(value["install_docker"], true);
        assert_eq!(value["hostname"], "hub");
        request.action = DeploymentAction::Update;
        let value: serde_json::Value =
            serde_json::from_slice(&request.stdin_json().unwrap()).unwrap();
        assert!(value.get("install_docker").is_none());
    }
    #[test]
    fn owner_setup_requests_carry_a_mode_and_never_a_credential() {
        let mut request = DeploymentRequest {
            action: DeploymentAction::OwnerSetup,
            deployment_id: Uuid::new_v4(),
            hostname: "hub".into(),
            hub_image: DEFAULT_IMAGE.into(),
            image_archive: None,
            image_archive_sha256: None,
            allow_session_interruptions: false,
            username: "owner".into(),
            owner_mode: OwnerSetupMode::Setup,
        };
        let value: serde_json::Value =
            serde_json::from_slice(&request.stdin_json().unwrap()).unwrap();
        assert_eq!(value["action"], "owner_setup");
        assert_eq!(value["mode"], "setup");
        assert_eq!(value["username"], "owner");
        assert_eq!(value["protocol_version"], 1);
        assert!(value.get("password").is_none());
        request.owner_mode = OwnerSetupMode::Reset;
        let value: serde_json::Value =
            serde_json::from_slice(&request.stdin_json().unwrap()).unwrap();
        assert_eq!(value["mode"], "reset");
        assert!(value.get("username").is_none());
        request.owner_mode = OwnerSetupMode::Setup;
        request.username = "not valid".into();
        assert!(request.stdin_json().is_err());
        request.action = DeploymentAction::Uninstall;
        let value: serde_json::Value =
            serde_json::from_slice(&request.stdin_json().unwrap()).unwrap();
        assert!(value.get("mode").is_none());
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
