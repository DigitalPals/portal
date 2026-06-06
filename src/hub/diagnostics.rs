use chrono::{DateTime, Utc};
use url::Url;

use crate::config::settings::PortalHubSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticStatus {
    Pass,
    Warning,
    Fail,
}

impl DiagnosticStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pass => "Pass",
            Self::Warning => "Check",
            Self::Fail => "Fail",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PortalHubDiagnosticCheck {
    pub name: String,
    pub status: DiagnosticStatus,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct PortalHubDiagnosticsReport {
    pub checked_at: DateTime<Utc>,
    pub checks: Vec<PortalHubDiagnosticCheck>,
}

impl PortalHubDiagnosticsReport {
    pub fn new() -> Self {
        Self {
            checked_at: Utc::now(),
            checks: Vec::new(),
        }
    }

    pub fn pass_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == DiagnosticStatus::Pass)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == DiagnosticStatus::Warning)
            .count()
    }

    pub fn fail_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == DiagnosticStatus::Fail)
            .count()
    }

    pub fn summary(&self) -> String {
        let failures = self.fail_count();
        let warnings = self.warning_count();
        if failures > 0 {
            format!("{failures} failed, {warnings} need review")
        } else if warnings > 0 {
            format!("{warnings} need review")
        } else {
            "All checks passed".to_string()
        }
    }

    fn push(
        &mut self,
        name: impl Into<String>,
        status: DiagnosticStatus,
        detail: impl Into<String>,
    ) {
        self.checks.push(PortalHubDiagnosticCheck {
            name: name.into(),
            status,
            detail: detail.into(),
        });
    }

    fn pass(&mut self, name: impl Into<String>, detail: impl Into<String>) {
        self.push(name, DiagnosticStatus::Pass, detail);
    }

    fn warn(&mut self, name: impl Into<String>, detail: impl Into<String>) {
        self.push(name, DiagnosticStatus::Warning, detail);
    }

    fn fail(&mut self, name: impl Into<String>, detail: impl Into<String>) {
        self.push(name, DiagnosticStatus::Fail, detail);
    }
}

pub async fn run_portal_hub_diagnostics(settings: PortalHubSettings) -> PortalHubDiagnosticsReport {
    let mut report = PortalHubDiagnosticsReport::new();
    let hub_url = settings.effective_web_url();

    if let Err(error) = validate_hub_url(&hub_url) {
        report.fail("Configuration", error);
        report.warn(
            "Portal Hub info",
            "Skipped because the Hub URL is not valid.",
        );
        report.warn(
            "Authentication",
            "Skipped because the Hub URL is not valid.",
        );
        report.warn("Sync API", "Skipped because the Hub URL is not valid.");
        report.warn("Session API", "Skipped because the Hub URL is not valid.");
        report.warn(
            "Terminal WebSocket",
            "Skipped because the Hub URL is not valid.",
        );
        check_local_vault_state(&settings, None, &mut report);
        return report;
    }

    if settings.enabled {
        report.pass(
            "Configuration",
            format!("Portal Hub is enabled and points to {hub_url}."),
        );
    } else {
        report.warn(
            "Configuration",
            format!("Portal Hub URL is {hub_url}, but persistent sessions are disabled locally."),
        );
    }

    let info = match crate::hub::auth::fetch_hub_info(&settings).await {
        Ok(info) => {
            report.pass(
                "Portal Hub info",
                format!(
                    "Hub {} reports API version {}.",
                    display_version(&info.version),
                    info.api_version
                ),
            );
            Some(info)
        }
        Err(error) => {
            report.fail("Portal Hub info", error);
            None
        }
    };

    check_capabilities(info.as_ref(), &mut report);

    let authenticated = match crate::hub::auth::refresh_access_token(&hub_url).await {
        Ok(Some(_)) => {
            report.pass("Authentication", "OAuth tokens refreshed successfully.");
            true
        }
        Ok(None) => {
            report.fail("Authentication", "Portal Hub is not authenticated.");
            false
        }
        Err(error) => {
            report.fail("Authentication", error);
            false
        }
    };

    check_sync_api(&settings, info.as_ref(), authenticated, &mut report).await;
    check_session_api(&settings, authenticated, &mut report).await;
    check_terminal_websocket(&settings, info.as_ref(), authenticated, &mut report).await;
    check_local_vault_state(&settings, info.as_ref(), &mut report);

    report
}

fn validate_hub_url(hub_url: &str) -> Result<(), String> {
    if hub_url.trim().is_empty() {
        return Err("Enter a Portal Hub URL or host first.".to_string());
    }
    let url = Url::parse(hub_url).map_err(|error| format!("Portal Hub URL is invalid: {error}"))?;
    match url.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(format!(
            "Portal Hub URL must use http:// or https://, not {scheme}://."
        )),
    }
}

fn check_capabilities(
    info: Option<&crate::hub::auth::HubInfo>,
    report: &mut PortalHubDiagnosticsReport,
) {
    let Some(info) = info else {
        report.warn(
            "Capabilities",
            "Skipped because Portal could not read Hub info.",
        );
        return;
    };

    let mut missing = Vec::new();
    if info.api_version < 2 {
        missing.push(format!("API version {} is older than 2", info.api_version));
    }
    if !info.capabilities.web_proxy {
        missing.push("web proxy support is missing".to_string());
    }
    if !info.capabilities.sync_v2 {
        missing.push("sync v2 support is missing".to_string());
    }

    if missing.is_empty() {
        report.pass(
            "Capabilities",
            format!(
                "web proxy: {}, sync v2: {}, sync events: {}, key vault: {}, vault enrollment: {}",
                on_off(info.capabilities.web_proxy),
                on_off(info.capabilities.sync_v2),
                on_off(info.capabilities.sync_events),
                on_off(info.capabilities.key_vault),
                on_off(info.capabilities.vault_enrollment),
            ),
        );
    } else {
        report.fail("Capabilities", missing.join("; "));
    }
}

async fn check_sync_api(
    settings: &PortalHubSettings,
    info: Option<&crate::hub::auth::HubInfo>,
    authenticated: bool,
    report: &mut PortalHubDiagnosticsReport,
) {
    if !authenticated {
        report.warn(
            "Sync API",
            "Skipped because Portal Hub is not authenticated.",
        );
        report.warn(
            "Sync events",
            "Skipped because Portal Hub is not authenticated.",
        );
        return;
    }

    if info.is_some_and(|info| !info.capabilities.sync_v2) {
        report.warn(
            "Sync API",
            "Skipped because Hub does not advertise sync v2.",
        );
        report.warn(
            "Sync events",
            "Skipped because Hub does not advertise sync v2.",
        );
        return;
    }

    match crate::hub::sync::http_sync_v2_get(settings).await {
        Ok(response) => report.pass(
            "Sync API",
            format!("Read {} service state(s).", response.services.len()),
        ),
        Err(error) => report.fail("Sync API", error),
    }

    if info.is_some_and(|info| !info.capabilities.sync_events) {
        report.warn(
            "Sync events",
            "Hub does not advertise sync events; Portal will rely on startup, local-change, and manual sync.",
        );
        return;
    }

    match crate::hub::sync::check_sync_revision_events(settings).await {
        Ok(()) => report.pass(
            "Sync events",
            "Authenticated event stream opened successfully.",
        ),
        Err(error) => report.fail("Sync events", error),
    }
}

async fn check_session_api(
    settings: &PortalHubSettings,
    authenticated: bool,
    report: &mut PortalHubDiagnosticsReport,
) {
    if !authenticated {
        report.warn(
            "Session API",
            "Skipped because Portal Hub is not authenticated.",
        );
        return;
    }

    match crate::proxy::list_active_sessions(settings).await {
        Ok(sessions) => report.pass(
            "Session API",
            format!("Listed {} active session(s).", sessions.len()),
        ),
        Err(error) => report.fail("Session API", error),
    }
}

async fn check_terminal_websocket(
    settings: &PortalHubSettings,
    info: Option<&crate::hub::auth::HubInfo>,
    authenticated: bool,
    report: &mut PortalHubDiagnosticsReport,
) {
    if !authenticated {
        report.warn(
            "Terminal WebSocket",
            "Skipped because Portal Hub is not authenticated.",
        );
        return;
    }
    if info.is_some_and(|info| !info.capabilities.web_proxy) {
        report.warn(
            "Terminal WebSocket",
            "Skipped because Hub does not advertise web proxy support.",
        );
        return;
    }

    match crate::proxy::check_terminal_websocket(settings).await {
        Ok(()) => report.pass(
            "Terminal WebSocket",
            "Authenticated terminal WebSocket handshake completed.",
        ),
        Err(error) => report.fail("Terminal WebSocket", error),
    }
}

fn check_local_vault_state(
    settings: &PortalHubSettings,
    info: Option<&crate::hub::auth::HubInfo>,
    report: &mut PortalHubDiagnosticsReport,
) {
    if !settings.key_vault_enabled {
        report.warn("Local vault", "Key vault sync is disabled locally.");
        return;
    }
    if info.is_some_and(|info| !info.capabilities.key_vault) {
        report.fail("Local vault", "Hub does not advertise key vault support.");
        return;
    }

    let vault = match crate::hub::vault::HubVaultConfig::load() {
        Ok(vault) => vault,
        Err(error) => {
            report.fail("Local vault", error);
            return;
        }
    };

    if vault.keys.is_empty() && vault.secrets.is_empty() {
        report.pass(
            "Local vault",
            "Vault file is readable; no encrypted items require unlock.",
        );
        return;
    }

    match crate::hub::vault::load_stored_vault_secret() {
        Ok(Some(_)) => report.pass(
            "Local vault",
            "Vault file and OS keychain secret are available.",
        ),
        Ok(None) => report.fail(
            "Local vault",
            "Vault has encrypted items, but no OS keychain vault secret was found.",
        ),
        Err(error) => report.fail("Local vault", error),
    }
}

fn display_version(version: &str) -> &str {
    if version.trim().is_empty() {
        "with unknown version"
    } else {
        version
    }
}

const fn on_off(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_summary_prefers_failures() {
        let mut report = PortalHubDiagnosticsReport::new();
        report.pass("A", "ok");
        report.warn("B", "look");
        report.fail("C", "bad");

        assert_eq!(report.pass_count(), 1);
        assert_eq!(report.warning_count(), 1);
        assert_eq!(report.fail_count(), 1);
        assert_eq!(report.summary(), "1 failed, 1 need review");
    }

    #[test]
    fn hub_url_must_be_http_or_https() {
        assert!(validate_hub_url("https://hub.example.test").is_ok());
        assert!(validate_hub_url("http://127.0.0.1:8080").is_ok());
        assert!(validate_hub_url("ssh://hub.example.test").is_err());
        assert!(validate_hub_url("").is_err());
    }
}
