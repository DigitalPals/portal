//! Passkey-only owner authentication.
//!
//! The web never creates an owner by itself. The first passkey, extra
//! passkeys and recovery all start from a one-time setup token that only the
//! local service administrator can mint (`portal-hub owner-setup`), so control
//! of the server over SSH is the root of trust. Portal desktop mints the token
//! through its deployment connection and opens `/setup#token=…`; the token sits
//! in the URL fragment, which browsers never send to the server or in Referer.
use super::*;
use std::time::Instant;
use webauthn_rs::prelude::{
    CredentialID, Passkey, PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential, Webauthn, WebauthnBuilder,
};

pub(super) const SETUP_TOKEN_TTL_MINUTES: i64 = 30;
const FLOW_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_PENDING_FLOWS: usize = 64;

/// What a setup token allows: enrolling the first passkey (creating the owner
/// when there is none), adding another passkey, or replacing all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum SetupMode {
    Setup,
    Add,
    Reset,
}

impl SetupMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Add => "add",
            Self::Reset => "reset",
        }
    }
    fn parse(value: &str) -> Result<Self> {
        match value {
            "setup" => Ok(Self::Setup),
            "add" => Ok(Self::Add),
            "reset" => Ok(Self::Reset),
            _ => bail!("invalid setup mode"),
        }
    }
}

/// In-memory WebAuthn ceremony state, bounded in size and age.
#[derive(Default)]
pub(super) struct Flows {
    registrations: HashMap<String, PendingRegistration>,
    authentications: HashMap<String, PendingAuthentication>,
}

struct PendingRegistration {
    created: Instant,
    token_hash: String,
    mode: SetupMode,
    user_id: Uuid,
    state: PasskeyRegistration,
}

struct PendingAuthentication {
    created: Instant,
    user_id: String,
    query: AuthorizeQuery,
    state: PasskeyAuthentication,
}

fn insert_bounded<T>(
    map: &mut HashMap<String, T>,
    key: String,
    value: T,
    age: impl Fn(&T) -> Instant,
) {
    map.retain(|_, flow| age(flow).elapsed() < FLOW_TTL);
    while map.len() >= MAX_PENDING_FLOWS {
        let Some(oldest) = map
            .iter()
            .min_by_key(|(_, flow)| age(flow))
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        map.remove(&oldest);
    }
    map.insert(key, value);
}

#[derive(Debug, Deserialize)]
pub(super) struct SetupTokenRequest {
    pub(super) token: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct SetupFinishRequest {
    flow_id: String,
    credential: RegisterPublicKeyCredential,
    #[serde(default)]
    oauth: Option<AuthorizeQuery>,
}

#[derive(Debug, Deserialize)]
pub(super) struct LoginStartRequest {
    oauth: AuthorizeQuery,
}

#[derive(Debug, Deserialize)]
pub(super) struct LoginFinishRequest {
    flow_id: String,
    credential: PublicKeyCredential,
}

#[derive(Debug, Serialize)]
pub(super) struct CeremonyStart {
    flow_id: String,
    public_key: Value,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(super) struct SetupInfo {
    username: String,
    mode: SetupMode,
}

#[derive(Debug, Serialize)]
pub(super) struct SetupFinished {
    ok: bool,
    redirect_uri: Option<String>,
}

/// The single Hub account and how many passkeys it has.
pub(super) struct Owner {
    pub(super) id: String,
    pub(super) username: String,
    pub(super) passkeys: i64,
}

pub(super) fn load_owner(db: &Connection) -> Result<Option<Owner>> {
    let users = db
        .prepare("SELECT id, username FROM users ORDER BY created_at, id")?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let (id, username) = match users.as_slice() {
        [] => return Ok(None),
        [owner] => owner.clone(),
        _ => bail!("this Hub has more than one account; passkey setup supports one owner"),
    };
    let passkeys = db.query_row(
        "SELECT COUNT(*) FROM passkeys WHERE user_id = ?1",
        [&id],
        |row| row.get(0),
    )?;
    Ok(Some(Owner {
        id,
        username,
        passkeys,
    }))
}

/// The owner can sign in: there is an account with at least one passkey.
pub(super) fn owner_ready(db: &Connection) -> bool {
    matches!(load_owner(db), Ok(Some(owner)) if owner.passkeys > 0)
}

pub(super) fn owner_ready_state(state: &AppState) -> bool {
    state.db.lock().is_ok_and(|db| owner_ready(&db))
}

/// Mint a one-time setup token, replacing any earlier unused one.
pub(super) fn mint_setup_token(
    db: &Connection,
    mode: SetupMode,
    username: Option<&str>,
) -> Result<(String, SetupInfo, DateTime<Utc>)> {
    let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)?;
    let owner = load_owner(&tx)?;
    let username = match (mode, &owner) {
        (SetupMode::Setup, None) => {
            let username =
                username.ok_or_else(|| anyhow!("choose an owner account name with --username"))?;
            validate_username(username)?;
            username.trim().to_string()
        }
        (SetupMode::Setup, Some(owner)) if owner.passkeys == 0 => owner.username.clone(),
        (SetupMode::Setup, Some(_)) => {
            bail!("the owner already has a passkey; use mode add or reset")
        }
        (SetupMode::Add | SetupMode::Reset, Some(owner)) => owner.username.clone(),
        (SetupMode::Add | SetupMode::Reset, None) => {
            bail!("this Hub has no owner yet; use mode setup")
        }
    };
    let token = random_token();
    let now = Utc::now();
    let expires_at = now + ChronoDuration::minutes(SETUP_TOKEN_TTL_MINUTES);
    tx.execute("DELETE FROM owner_setup_tokens", [])?;
    tx.execute(
        "INSERT INTO owner_setup_tokens (token_hash, mode, username, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![token_hash(&token), mode.as_str(), username, now.to_rfc3339(), expires_at.to_rfc3339()],
    )?;
    audit_db(
        &tx,
        "owner_setup_link_created",
        owner.as_ref().map(|owner| owner.id.as_str()),
        json!({"mode": mode.as_str()}),
    )?;
    tx.commit()?;
    Ok((token, SetupInfo { username, mode }, expires_at))
}

fn setup_grant(db: &Connection, token_hash: &str) -> Result<SetupInfo> {
    let expired =
        || anyhow!("this setup link has expired or was already used; start again from Portal");
    let (mode, username, expires_at): (String, String, String) = db
        .query_row(
            "SELECT mode, username, expires_at FROM owner_setup_tokens WHERE token_hash = ?1",
            [token_hash],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or_else(expired)?;
    if DateTime::parse_from_rfc3339(&expires_at)? <= Utc::now() {
        return Err(expired());
    }
    Ok(SetupInfo {
        username,
        mode: SetupMode::parse(&mode)?,
    })
}

pub fn owner_setup_cli(
    state_dir: &Path,
    as_json: bool,
    mode: SetupMode,
    username: Option<String>,
) -> Result<()> {
    local_bootstrap_only()?;
    let db = open_db(state_dir)?;
    let (token, info, expires_at) = mint_setup_token(&db, mode, username.as_deref())?;
    let setup_url = std::env::var("PORTAL_HUB_PUBLIC_URL")
        .ok()
        .and_then(|url| canonicalize_public_url(&url).ok())
        .filter(|url| webauthn_for_public_url(url).is_ok())
        .map(|url| format!("{url}/setup#token={token}"));
    if as_json {
        println!(
            "{}",
            json!({
                "token": token,
                "username": info.username,
                "mode": info.mode,
                "expires_at": expires_at.to_rfc3339(),
                "setup_url": setup_url,
            })
        );
    } else {
        match setup_url {
            Some(url) => println!(
                "Open this link within {SETUP_TOKEN_TTL_MINUTES} minutes to create the passkey for {}:\n{url}",
                info.username
            ),
            None => println!(
                "Open <your Hub HTTPS address>/setup#token={token} within {SETUP_TOKEN_TTL_MINUTES} minutes to create the passkey for {}.",
                info.username
            ),
        }
    }
    Ok(())
}

pub(super) fn webauthn(state: &AppState) -> Result<Webauthn> {
    webauthn_for_public_url(&state.public_url)
}

/// Passkeys belong to the Hub's HTTPS name (its Tailscale MagicDNS name for a
/// managed deployment). Only localhost development names may use plain HTTP.
pub(super) fn webauthn_for_public_url(public_url: &str) -> Result<Webauthn> {
    let origin = Url::parse(public_url.trim_end_matches('/'))?;
    let host = origin
        .host_str()
        .ok_or_else(|| anyhow!("Portal Hub public URL must include a host"))?;
    if origin.scheme() != "https" && !is_local_passkey_host(host) {
        bail!("passkeys need the Hub's HTTPS address; finish private HTTPS setup in Portal first");
    }
    Ok(WebauthnBuilder::new(host, &origin)?
        .rp_name("Portal Hub")
        .allow_any_port(true)
        .build()?)
}

fn is_local_passkey_host(host: &str) -> bool {
    host == "localhost" || host.ends_with(".localhost")
}

/// Ask passkey providers for a discoverable credential, which is what syncing
/// password managers store, without excluding security keys that cannot.
fn prefer_discoverable(public_key: &mut Value) {
    if let Some(selection) = public_key
        .get_mut("publicKey")
        .and_then(|options| options.get_mut("authenticatorSelection"))
        .and_then(Value::as_object_mut)
    {
        selection.insert("residentKey".into(), json!("preferred"));
        selection.insert("requireResidentKey".into(), json!(false));
    }
}

fn flows(state: &AppState) -> Result<std::sync::MutexGuard<'_, Flows>> {
    state
        .flows
        .lock()
        .map_err(|_| anyhow!("passkey state lock poisoned"))
}

pub(super) fn setup_info(state: &AppState, token: &str) -> Result<SetupInfo> {
    let db = state
        .db
        .lock()
        .map_err(|_| anyhow!("database lock poisoned"))?;
    setup_grant(&db, &token_hash(token))
}

pub(super) fn setup_start(state: &AppState, token: &str) -> Result<CeremonyStart> {
    let webauthn = webauthn(state)?;
    let token_hash = token_hash(token);
    let (info, user_id, exclude) = {
        let db = state
            .db
            .lock()
            .map_err(|_| anyhow!("database lock poisoned"))?;
        let info = setup_grant(&db, &token_hash)?;
        let owner = load_owner(&db)?;
        let user_id = match &owner {
            Some(owner) => Uuid::parse_str(&owner.id)?,
            None => Uuid::new_v4(),
        };
        // Adding the same authenticator twice would only confuse sign-in.
        let exclude = match (&owner, info.mode) {
            (Some(owner), SetupMode::Add) => Some(
                load_passkeys(&db, &owner.id)?
                    .into_iter()
                    .map(|(_, passkey)| passkey.cred_id().clone())
                    .collect::<Vec<CredentialID>>(),
            ),
            _ => None,
        };
        (info, user_id, exclude)
    };
    let (challenge, registration) =
        webauthn.start_passkey_registration(user_id, &info.username, &info.username, exclude)?;
    let flow_id = random_token();
    insert_bounded(
        &mut flows(state)?.registrations,
        flow_id.clone(),
        PendingRegistration {
            created: Instant::now(),
            token_hash,
            mode: info.mode,
            user_id,
            state: registration,
        },
        |flow| flow.created,
    );
    let mut public_key = serde_json::to_value(challenge)?;
    prefer_discoverable(&mut public_key);
    Ok(CeremonyStart {
        flow_id,
        public_key,
    })
}

pub(super) fn setup_finish(state: &AppState, request: SetupFinishRequest) -> Result<SetupFinished> {
    if let Some(oauth) = &request.oauth {
        validate_authorize_query(oauth)?;
    }
    let pending = flows(state)?
        .registrations
        .remove(&request.flow_id)
        .filter(|flow| flow.created.elapsed() < FLOW_TTL)
        .ok_or_else(|| anyhow!("passkey setup timed out; select Create passkey again"))?;
    let passkey = webauthn(state)?
        .finish_passkey_registration(&request.credential, &pending.state)
        .map_err(|error| anyhow!("the browser returned a passkey this Hub cannot use: {error}"))?;
    let db = state
        .db
        .lock()
        .map_err(|_| anyhow!("database lock poisoned"))?;
    let tx = Transaction::new_unchecked(&db, TransactionBehavior::Immediate)?;
    // Consume the link in the same transaction that uses it: a second tab,
    // an expired link or a link minted again in the meantime all fail here.
    let info = setup_grant(&tx, &pending.token_hash)?;
    tx.execute(
        "DELETE FROM owner_setup_tokens WHERE token_hash = ?1",
        [&pending.token_hash],
    )?;
    if info.mode != pending.mode {
        bail!("this setup link has changed; start again from Portal");
    }
    ensure_credential_is_unique(&tx, &passkey)?;
    let user_id = pending.user_id.to_string();
    match (info.mode, load_owner(&tx)?) {
        (SetupMode::Setup, None) => {
            insert_user(&tx, pending.user_id, &info.username, "")?;
            audit_db(
                &tx,
                "owner_created",
                Some(&user_id),
                json!({"username": info.username, "auth_method": "passkey"}),
            )?;
        }
        (_, Some(owner)) if owner.id != user_id => {
            bail!("the Hub owner changed during setup; start again from Portal")
        }
        (SetupMode::Setup, Some(owner)) if owner.passkeys > 0 => {
            bail!("the owner already has a passkey; use Add a passkey in Portal")
        }
        (SetupMode::Reset, Some(_)) => {
            // Recovery: every old passkey and every signed-in device is revoked.
            tx.execute("DELETE FROM passkeys WHERE user_id = ?1", [&user_id])?;
            tx.execute("DELETE FROM access_tokens WHERE user_id = ?1", [&user_id])?;
            tx.execute("DELETE FROM refresh_tokens WHERE user_id = ?1", [&user_id])?;
            tx.execute("DELETE FROM auth_codes WHERE user_id = ?1", [&user_id])?;
            audit_db(&tx, "owner_passkeys_reset", Some(&user_id), json!({}))?;
        }
        (SetupMode::Setup | SetupMode::Add, Some(_)) => {}
        (SetupMode::Add | SetupMode::Reset, None) => bail!("this Hub has no owner yet"),
    }
    insert_passkey(&tx, &user_id, &passkey)?;
    // Passkey-only: a password from an older Hub version stops working.
    tx.execute(
        "UPDATE users SET password_hash = '' WHERE id = ?1",
        [&user_id],
    )?;
    audit_db(
        &tx,
        "passkey_registered",
        Some(&user_id),
        json!({"mode": info.mode.as_str()}),
    )?;
    let redirect_uri = match &request.oauth {
        Some(oauth) => Some(issue_authorization_code(
            &tx,
            &user_id,
            oauth,
            "passkey_setup",
        )?),
        None => None,
    };
    tx.commit()?;
    Ok(SetupFinished {
        ok: true,
        redirect_uri,
    })
}

pub(super) fn login_start(state: &AppState, request: LoginStartRequest) -> Result<CeremonyStart> {
    validate_authorize_query(&request.oauth)?;
    let webauthn = webauthn(state)?;
    let (owner, passkeys) = {
        let db = state
            .db
            .lock()
            .map_err(|_| anyhow!("database lock poisoned"))?;
        let owner = load_owner(&db)?
            .filter(|owner| owner.passkeys > 0)
            .ok_or_else(|| anyhow!("this Hub has no owner passkey yet; finish setup in Portal"))?;
        let passkeys = load_passkeys(&db, &owner.id)?
            .into_iter()
            .map(|(_, passkey)| passkey)
            .collect::<Vec<_>>();
        (owner, passkeys)
    };
    let (challenge, authentication) = webauthn.start_passkey_authentication(&passkeys)?;
    let flow_id = random_token();
    insert_bounded(
        &mut flows(state)?.authentications,
        flow_id.clone(),
        PendingAuthentication {
            created: Instant::now(),
            user_id: owner.id,
            query: request.oauth,
            state: authentication,
        },
        |flow| flow.created,
    );
    Ok(CeremonyStart {
        flow_id,
        public_key: serde_json::to_value(challenge)?,
    })
}

pub(super) fn login_finish(
    state: &AppState,
    request: LoginFinishRequest,
) -> Result<LoginFinishResponse> {
    let pending = flows(state)?
        .authentications
        .remove(&request.flow_id)
        .filter(|flow| flow.created.elapsed() < FLOW_TTL)
        .ok_or_else(|| anyhow!("sign-in timed out; select Sign in with passkey again"))?;
    let db = state
        .db
        .lock()
        .map_err(|_| anyhow!("database lock poisoned"))?;
    let result =
        match webauthn(state)?.finish_passkey_authentication(&request.credential, &pending.state) {
            Ok(result) => result,
            Err(error) => {
                audit_db(
                    &db,
                    "login_failed",
                    Some(&pending.user_id),
                    json!({"reason": "passkey_rejected"}),
                )?;
                bail!("the passkey was not accepted: {error}");
            }
        };
    let tx = Transaction::new_unchecked(&db, TransactionBehavior::Immediate)?;
    let mut matched = None;
    for (id, mut passkey) in load_passkeys(&tx, &pending.user_id)? {
        if passkey.cred_id() == result.cred_id() {
            if result.needs_update() && passkey.update_credential(&result).is_some() {
                tx.execute(
                    "UPDATE passkeys SET passkey = ?1 WHERE id = ?2",
                    params![serde_json::to_string(&passkey)?, id],
                )?;
            }
            matched = Some(id);
            break;
        }
    }
    // A reset can remove the passkey between the two halves of the ceremony.
    let Some(passkey_id) = matched else {
        audit_db(
            &tx,
            "login_failed",
            Some(&pending.user_id),
            json!({"reason": "unknown_passkey"}),
        )?;
        tx.commit()?;
        bail!("this passkey is no longer registered to the Hub owner");
    };
    tx.execute(
        "UPDATE passkeys SET last_used_at = ?1 WHERE id = ?2",
        params![Utc::now().to_rfc3339(), passkey_id],
    )?;
    let redirect_uri = issue_authorization_code(&tx, &pending.user_id, &pending.query, "passkey")?;
    tx.commit()?;
    Ok(LoginFinishResponse { redirect_uri })
}

/// One OAuth authorization code for a verified owner, returned as the
/// client's redirect target with the original state.
fn issue_authorization_code(
    db: &Connection,
    user_id: &str,
    query: &AuthorizeQuery,
    auth_method: &str,
) -> Result<String> {
    let code = random_token();
    let expires_at = (Utc::now() + ChronoDuration::minutes(AUTH_CODE_TTL_MINUTES)).to_rfc3339();
    db.execute(
        "INSERT INTO auth_codes (code, user_id, client_id, redirect_uri, code_challenge, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![code, user_id, query.client_id, query.redirect_uri, query.code_challenge, expires_at],
    )?;
    audit_db(
        db,
        "login_success",
        Some(user_id),
        json!({"client_id": query.client_id, "auth_method": auth_method}),
    )?;
    Ok(format!(
        "{}?code={}&state={}",
        query.redirect_uri,
        urlencoding::encode(&code),
        urlencoding::encode(&query.state)
    ))
}

fn insert_passkey(db: &Connection, user_id: &str, passkey: &Passkey) -> Result<()> {
    db.execute(
        "INSERT INTO passkeys (id, user_id, passkey, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            Uuid::new_v4().to_string(),
            user_id,
            serde_json::to_string(passkey)?,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

fn load_passkeys(db: &Connection, user_id: &str) -> Result<Vec<(String, Passkey)>> {
    let rows = db
        .prepare("SELECT id, passkey FROM passkeys WHERE user_id = ?1")?
        .query_map([user_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, passkey)| Ok((id, serde_json::from_str(&passkey)?)))
        .collect()
}

fn ensure_credential_is_unique(db: &Connection, passkey: &Passkey) -> Result<()> {
    let existing = db
        .prepare("SELECT passkey FROM passkeys")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for json in existing {
        let existing: Passkey = serde_json::from_str(&json)?;
        if existing.cred_id() == passkey.cred_id() {
            bail!("this passkey is already registered to the Hub");
        }
    }
    Ok(())
}

pub(super) async fn setup_page() -> Response {
    Html(page(
        "Create Your Portal Hub Passkey",
        &format!(
            r#"<section class="panel setup-panel">
            <p class="eyebrow" id="setup-eyebrow">Owner passkey</p>
            <h1 id="setup-title">Create your passkey.</h1>
            <p class="lead" id="setup-lead">Portal Hub signs you in with a passkey instead of a password. Your browser or password manager stores it.</p>
            <div id="setup-error" class="error" hidden></div>
            <form id="setup-form" class="flow" hidden>
              <p class="account-line">Account: <strong id="setup-account"></strong></p>
              <button type="submit" id="setup-button">Create passkey</button>
            </form>
            {}
          </section>"#,
            sync_callout()
        ),
    ))
    .into_response()
}

pub(super) fn sign_in_page() -> String {
    page(
        "Sign In To Portal Hub",
        &format!(
            r#"<section class="panel auth-panel">
            <p class="eyebrow">Portal sign-in</p>
            <h1>Sign in with your passkey.</h1>
            <p class="lead">Your browser or password manager confirms the passkey for this Hub, then returns you to Portal.</p>
            <div id="login-error" class="error" hidden></div>
            <form id="login-form" class="flow">
              <button type="submit" id="login-button">Sign in with passkey</button>
            </form>
            {}
          </section>"#,
            recovery_callout()
        ),
    )
}

pub(super) fn finish_setup_in_portal_page() -> String {
    page(
        "Finish Portal Hub Setup",
        &format!(
            r#"<section class="panel">
            <p class="eyebrow">Setup</p>
            <h1>Finish setup in Portal.</h1>
            <p class="lead">This Hub has no owner passkey yet. In Portal, open Settings, then Portal Hub, and select Deploy Hub or Resume setup for this server. Portal creates the passkey with you.</p>
            {}
          </section>"#,
            recovery_callout()
        ),
    )
}

fn sync_callout() -> &'static str {
    r#"<div class="security-callout">
          <span class="security-icon" aria-hidden="true"></span>
          <div>
            <strong>One passkey for all your devices.</strong>
            <p>Save it in a password manager that syncs, such as Google Password Manager, iCloud Keychain, 1Password or Bitwarden, or on a security key. Then the same passkey signs you in on your phone and other computers.</p>
          </div>
        </div>"#
}

fn recovery_callout() -> &'static str {
    r#"<div class="security-callout">
          <span class="security-icon" aria-hidden="true"></span>
          <div>
            <strong>Lost your passkey?</strong>
            <p>In Portal, open the Hub's server under Set up on a server and select Reset owner passkey. On a server you manage yourself, run <code>portal-hub owner-setup --mode reset</code>.</p>
          </div>
        </div>"#
}

pub(super) fn passkey_script() -> &'static str {
    r#"
      function showError(id, message) {
        const node = document.getElementById(id);
        if (!node) return;
        node.textContent = message;
        node.hidden = false;
      }
      function clearError(id) {
        const node = document.getElementById(id);
        if (!node) return;
        node.textContent = "";
        node.hidden = true;
      }
      function b64urlToBuffer(value) {
        const base64 = value.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(value.length / 4) * 4, "=");
        const binary = atob(base64);
        const bytes = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
        return bytes.buffer;
      }
      function bufferToB64url(buffer) {
        const bytes = new Uint8Array(buffer);
        let binary = "";
        for (const byte of bytes) binary += String.fromCharCode(byte);
        return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
      }
      function creationOptionsFromJSON(options) {
        options = options.publicKey || options;
        options.challenge = b64urlToBuffer(options.challenge);
        options.user.id = b64urlToBuffer(options.user.id);
        if (options.excludeCredentials) {
          options.excludeCredentials = options.excludeCredentials.map((credential) => ({
            ...credential,
            id: b64urlToBuffer(credential.id),
          }));
        }
        return options;
      }
      function requestOptionsFromJSON(options) {
        options = options.publicKey || options;
        options.challenge = b64urlToBuffer(options.challenge);
        if (options.allowCredentials) {
          options.allowCredentials = options.allowCredentials.map((credential) => ({
            ...credential,
            id: b64urlToBuffer(credential.id),
          }));
        }
        return options;
      }
      function credentialToJSON(credential) {
        const response = credential.response;
        const json = {
          id: credential.id,
          rawId: bufferToB64url(credential.rawId),
          type: credential.type,
          response: { clientDataJSON: bufferToB64url(response.clientDataJSON) },
          extensions: {},
        };
        if (response.attestationObject) json.response.attestationObject = bufferToB64url(response.attestationObject);
        if (response.authenticatorData) json.response.authenticatorData = bufferToB64url(response.authenticatorData);
        if (response.signature) json.response.signature = bufferToB64url(response.signature);
        if (response.userHandle) json.response.userHandle = bufferToB64url(response.userHandle);
        if (typeof response.getTransports === "function") json.response.transports = response.getTransports();
        return json;
      }
      async function postJSON(url, body) {
        const response = await fetch(url, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(body),
        });
        const json = await response.json().catch(() => ({}));
        if (!response.ok) throw new Error(json.error || "Request failed");
        return json;
      }
      function oauthQuery() {
        const params = new URLSearchParams(window.location.search);
        return params.has("client_id") ? Object.fromEntries(params.entries()) : null;
      }
      function friendly(error) {
        if (error && error.name === "NotAllowedError") {
          return "The passkey request was cancelled or timed out. Try again when you are ready.";
        }
        if (error && error.name === "InvalidStateError") {
          return "This authenticator already has a passkey for this Hub. Choose another one.";
        }
        return (error && error.message) || String(error);
      }
      async function initSetup() {
        const form = document.getElementById("setup-form");
        const submit = document.getElementById("setup-button");
        // The token is a one-time secret: keep it out of history and bookmarks.
        const token = new URLSearchParams(window.location.hash.slice(1)).get("token");
        history.replaceState(null, "", window.location.pathname + window.location.search);
        if (!token) {
          showError("setup-error", "This setup link is incomplete. Start setup again from Portal.");
          return;
        }
        let info;
        try {
          info = await postJSON("/webauthn/setup/info", { token });
        } catch (error) {
          showError("setup-error", friendly(error));
          return;
        }
        document.getElementById("setup-account").textContent = info.username;
        if (info.mode === "add") {
          document.getElementById("setup-title").textContent = "Add a passkey.";
          document.getElementById("setup-lead").textContent = "Add another passkey for this Hub, for example a security key or a second password manager. Your existing passkeys keep working.";
        } else if (info.mode === "reset") {
          document.getElementById("setup-eyebrow").textContent = "Owner recovery";
          document.getElementById("setup-title").textContent = "Replace your passkeys.";
          document.getElementById("setup-lead").textContent = "The new passkey replaces all earlier ones. Every other device is signed out and signs in again with the new passkey.";
        }
        form.hidden = false;
        submit.focus();
        form.addEventListener("submit", async (event) => {
          event.preventDefault();
          clearError("setup-error");
          submit.disabled = true;
          submit.textContent = "Waiting for passkey...";
          try {
            const start = await postJSON("/webauthn/setup/start", { token });
            const credential = await navigator.credentials.create({ publicKey: creationOptionsFromJSON(start.public_key) });
            const finish = await postJSON("/webauthn/setup/finish", {
              flow_id: start.flow_id,
              credential: credentialToJSON(credential),
              oauth: oauthQuery(),
            });
            if (finish.redirect_uri) {
              document.querySelector(".setup-panel").innerHTML =
                '<p class="eyebrow">Passkey saved</p><h1>You are signed in.</h1><p class="lead">Return to Portal. You can close this tab.</p>';
              window.location.assign(finish.redirect_uri);
              return;
            }
            document.querySelector(".setup-panel").innerHTML =
              '<p class="eyebrow">Passkey saved</p><h1>Your passkey is ready.</h1><p class="lead">Sign in from Portal with this passkey. You can close this tab.</p>';
          } catch (error) {
            showError("setup-error", friendly(error));
            submit.disabled = false;
            submit.textContent = "Create passkey";
          }
        });
      }
      function initLogin() {
        const form = document.getElementById("login-form");
        const submit = document.getElementById("login-button");
        let running = false;
        async function signIn(automatic) {
          if (running) return;
          running = true;
          clearError("login-error");
          submit.disabled = true;
          submit.textContent = "Waiting for passkey...";
          try {
            const start = await postJSON("/webauthn/login/start", { oauth: oauthQuery() });
            const credential = await navigator.credentials.get({ publicKey: requestOptionsFromJSON(start.public_key) });
            const finish = await postJSON("/webauthn/login/finish", {
              flow_id: start.flow_id,
              credential: credentialToJSON(credential),
            });
            window.location.assign(finish.redirect_uri);
          } catch (error) {
            // An automatic prompt the person dismissed is not an error.
            if (!(automatic && error && error.name === "NotAllowedError")) {
              showError("login-error", friendly(error));
            }
            submit.disabled = false;
            submit.textContent = "Sign in with passkey";
          } finally {
            running = false;
          }
        }
        form.addEventListener("submit", (event) => {
          event.preventDefault();
          signIn(false);
        });
        signIn(true);
      }
      if (document.getElementById("setup-form")) initSetup();
      if (document.getElementById("login-form")) initLogin();
    "#
}

#[cfg(test)]
mod tests {
    use super::super::tests::test_state;
    use super::*;
    use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
    use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse};

    const ORIGIN: &str = "https://portal-hub.example.ts.net";

    /// A Hub on its Tailscale HTTPS name, like a managed deployment.
    fn hub() -> AppState {
        let mut state = test_state();
        state.public_url = ORIGIN.into();
        state
    }

    fn authenticator() -> WebauthnAuthenticator<SoftPasskey> {
        WebauthnAuthenticator::new(SoftPasskey::new(true))
    }

    fn oauth() -> AuthorizeQuery {
        AuthorizeQuery {
            response_type: "code".into(),
            client_id: DESKTOP_CLIENT_ID.into(),
            redirect_uri: "http://127.0.0.1:49152/callback".into(),
            code_challenge: "abcdefghijklmnopqrstuvwxyz123456".into(),
            code_challenge_method: "S256".into(),
            state: "state-state-state".into(),
        }
    }

    fn mint(state: &AppState, mode: SetupMode, username: Option<&str>) -> Result<String> {
        let db = state.db.lock().unwrap();
        mint_setup_token(&db, mode, username).map(|(token, _, _)| token)
    }

    /// The browser half of a registration, through the same JSON as the page.
    fn register(
        state: &AppState,
        device: &mut WebauthnAuthenticator<SoftPasskey>,
        token: &str,
        oauth: Option<AuthorizeQuery>,
    ) -> Result<SetupFinished> {
        let start = setup_start(state, token)?;
        let options: CreationChallengeResponse = serde_json::from_value(start.public_key)?;
        let credential = device
            .do_registration(Url::parse(ORIGIN)?, options)
            .map_err(|error| anyhow!("{error:?}"))?;
        setup_finish(
            state,
            serde_json::from_value(json!({
                "flow_id": start.flow_id,
                "credential": serde_json::to_value(credential)?,
                "oauth": oauth,
            }))?,
        )
    }

    fn sign_in(
        state: &AppState,
        device: &mut WebauthnAuthenticator<SoftPasskey>,
    ) -> Result<LoginFinishResponse> {
        let start = login_start(state, LoginStartRequest { oauth: oauth() })?;
        let options: RequestChallengeResponse = serde_json::from_value(start.public_key)?;
        let credential = device
            .do_authentication(Url::parse(ORIGIN)?, options)
            .map_err(|error| anyhow!("{error:?}"))?;
        login_finish(
            state,
            serde_json::from_value(json!({
                "flow_id": start.flow_id,
                "credential": serde_json::to_value(credential)?,
            }))?,
        )
    }

    fn count(state: &AppState, sql: &str) -> i64 {
        state
            .db
            .lock()
            .unwrap()
            .query_row(sql, [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn setup_link_creates_the_owner_and_signs_portal_in() {
        let state = hub();
        assert!(!owner_ready_state(&state));
        assert!(login_start(&state, LoginStartRequest { oauth: oauth() }).is_err());
        assert!(mint(&state, SetupMode::Setup, None).is_err());
        assert!(mint(&state, SetupMode::Add, Some("owner")).is_err());
        let token = mint(&state, SetupMode::Setup, Some("owner")).unwrap();
        assert_eq!(
            setup_info(&state, &token).unwrap(),
            SetupInfo {
                username: "owner".into(),
                mode: SetupMode::Setup
            }
        );
        let mut phone = authenticator();
        let finished = register(&state, &mut phone, &token, Some(oauth())).unwrap();
        let redirect = finished.redirect_uri.unwrap();
        assert!(redirect.starts_with("http://127.0.0.1:49152/callback?code="));
        assert!(redirect.ends_with("&state=state-state-state"));
        assert!(owner_ready_state(&state));
        // The link is single use.
        assert!(setup_info(&state, &token).is_err());
        assert!(register(&state, &mut authenticator(), &token, None).is_err());
        // The same passkey signs in afterwards; another device cannot.
        assert!(
            sign_in(&state, &mut phone)
                .unwrap()
                .redirect_uri
                .contains("code=")
        );
        assert!(sign_in(&state, &mut authenticator()).is_err());
        assert_eq!(count(&state, "SELECT COUNT(*) FROM users"), 1);
        assert!(mint(&state, SetupMode::Setup, Some("owner")).is_err());
    }

    #[test]
    fn add_keeps_existing_passkeys_and_reset_revokes_everything() {
        let state = hub();
        let mut first = authenticator();
        let token = mint(&state, SetupMode::Setup, Some("owner")).unwrap();
        register(&state, &mut first, &token, None).unwrap();
        let user_id: String = state
            .db
            .lock()
            .unwrap()
            .query_row("SELECT id FROM users", [], |row| row.get(0))
            .unwrap();
        issue_tokens(&state.db.lock().unwrap(), &user_id, DESKTOP_CLIENT_ID).unwrap();

        let mut second = authenticator();
        let token = mint(&state, SetupMode::Add, None).unwrap();
        register(&state, &mut second, &token, None).unwrap();
        assert!(sign_in(&state, &mut first).is_ok());
        assert!(sign_in(&state, &mut second).is_ok());
        assert_eq!(count(&state, "SELECT COUNT(*) FROM refresh_tokens"), 1);

        let mut replacement = authenticator();
        let token = mint(&state, SetupMode::Reset, None).unwrap();
        register(&state, &mut replacement, &token, None).unwrap();
        assert!(sign_in(&state, &mut first).is_err());
        assert!(sign_in(&state, &mut second).is_err());
        assert!(sign_in(&state, &mut replacement).is_ok());
        assert_eq!(count(&state, "SELECT COUNT(*) FROM passkeys"), 1);
        assert_eq!(count(&state, "SELECT COUNT(*) FROM refresh_tokens"), 0);
        assert_eq!(count(&state, "SELECT COUNT(*) FROM access_tokens"), 0);
    }

    #[test]
    fn a_new_link_replaces_the_old_one_and_expired_links_fail() {
        let state = hub();
        let old = mint(&state, SetupMode::Setup, Some("owner")).unwrap();
        let start = setup_start(&state, &old).unwrap();
        let new = mint(&state, SetupMode::Setup, Some("owner")).unwrap();
        assert!(setup_info(&state, &old).is_err());
        // A ceremony begun with the old link cannot finish with it.
        let options: CreationChallengeResponse = serde_json::from_value(start.public_key).unwrap();
        let credential = authenticator()
            .do_registration(Url::parse(ORIGIN).unwrap(), options)
            .unwrap();
        let finish = serde_json::from_value(json!({
            "flow_id": start.flow_id,
            "credential": serde_json::to_value(credential).unwrap(),
        }))
        .unwrap();
        assert!(setup_finish(&state, finish).is_err());
        assert_eq!(count(&state, "SELECT COUNT(*) FROM users"), 0);

        state
            .db
            .lock()
            .unwrap()
            .execute(
                "UPDATE owner_setup_tokens SET expires_at = ?1",
                [(Utc::now() - ChronoDuration::minutes(1)).to_rfc3339()],
            )
            .unwrap();
        assert!(setup_info(&state, &new).is_err());
        assert!(register(&state, &mut authenticator(), &new, None).is_err());
    }

    #[test]
    fn password_owner_from_an_older_hub_enrolls_a_passkey_and_loses_the_password() {
        let state = hub();
        let user_id = Uuid::new_v4();
        insert_user(
            &state.db.lock().unwrap(),
            user_id,
            "legacy",
            "$argon2id$old-hash",
        )
        .unwrap();
        assert!(!owner_ready_state(&state));
        // Setup keeps the existing account name, whatever Portal suggests.
        let token = mint(&state, SetupMode::Setup, Some("someone-else")).unwrap();
        assert_eq!(setup_info(&state, &token).unwrap().username, "legacy");
        let mut device = authenticator();
        register(&state, &mut device, &token, None).unwrap();
        let (count, hash): (i64, String) = state
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*), MAX(password_hash) FROM users",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((count, hash.as_str()), (1, ""));
        assert!(sign_in(&state, &mut device).is_ok());
    }

    #[test]
    fn passkeys_need_https_except_for_localhost_development() {
        assert!(webauthn_for_public_url("https://hub.example.ts.net").is_ok());
        assert!(webauthn_for_public_url("http://portal-hub.localhost:8080").is_ok());
        assert!(webauthn_for_public_url("http://127.0.0.1:8080").is_err());
        assert!(webauthn_for_public_url("http://hub.example.ts.net").is_err());
    }

    #[test]
    fn pending_ceremonies_are_bounded() {
        let mut map = HashMap::new();
        for i in 0..(MAX_PENDING_FLOWS * 2) {
            insert_bounded(&mut map, i.to_string(), Instant::now(), |at| *at);
        }
        assert_eq!(map.len(), MAX_PENDING_FLOWS);
        assert!(map.contains_key(&(MAX_PENDING_FLOWS * 2 - 1).to_string()));
    }

    #[tokio::test]
    async fn sign_in_page_never_offers_a_password_and_setup_waits_for_portal() {
        let state = hub();
        let response = super::super::authorize_page(State(state.clone()), Query(oauth())).await;
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("Finish setup in Portal"));
        let token = mint(&state, SetupMode::Setup, Some("owner")).unwrap();
        register(&state, &mut authenticator(), &token, None).unwrap();
        let response = super::super::authorize_page(State(state), Query(oauth())).await;
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("Sign in with passkey"));
        assert!(!body.contains("type=\"password\""));
    }
}
