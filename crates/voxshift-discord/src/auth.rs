//! Discord OAuth2 authorization — public client (no Client Secret, no
//! separate broker service), authorization code obtained via the Discord
//! RPC `AUTHORIZE` command.
//!
//! Two live attempts confirmed that Discord's `rpc`/`rpc.voice.*` scopes
//! are rejected with `invalid_scope` by the standard browser-based
//! `/oauth2/authorize` endpoint — those scopes are only grantable through
//! the RPC `AUTHORIZE` command, which shows its consent dialog natively
//! inside the running Discord desktop client (§6.2.1). "Public client"
//! still applies to the *token exchange* step: per Discord's own
//! definition, a public client is "unable to maintain the confidentiality
//! of client credentials... e.g. a desktop or mobile application", so the
//! subsequent code-for-token exchange at `/oauth2/token` is done with just
//! `client_id` — no Client Secret stored or transmitted anywhere.

use std::time::{Duration, SystemTime};

use base64::Engine as _;
use secrecy::{ExposeSecret, SecretString};
use sha2::Digest;
use uuid::Uuid;

use voxshift_core::token::{StoredTokenSet, TokenStore};

use crate::client::DiscordRpcClient;
use crate::error::DiscordError;

const TOKEN_URL: &str = "https://discord.com/api/oauth2/token";
const REVOKE_URL: &str = "https://discord.com/api/oauth2/token/revoke";

/// `rpc` + `identify` alone let the RPC connection authenticate, but
/// `GET_VOICE_SETTINGS`/`SET_VOICE_SETTINGS` additionally need the granular
/// voice scopes to actually return/change state. The web
/// `/oauth2/authorize` endpoint rejects `rpc.voice.*` with `invalid_scope`,
/// but the RPC `AUTHORIZE` command (a different, client-side validation
/// path) accepts the full documented RPC scope set.
const SCOPES: &[&str] = &["rpc", "identify", "rpc.voice.read", "rpc.voice.write"];

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub client_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("discord did not authorize this application")]
    NotAuthorized,
    #[error("this application is not yet approved for RPC (or the tester limit was reached)")]
    RpcNotApproved,
    #[error("refresh token is no longer valid; reauthorization is required")]
    ReauthorizationRequired,
    #[error("could not reach discord's oauth2 endpoint")]
    DiscordUnreachable,
    #[error("credential store error: {0}")]
    CredentialStore(#[from] voxshift_core::error::CoreError),
    #[error(transparent)]
    Client(#[from] DiscordError),
}

// ---------------------------------------------------------------------
// AUTHORIZE (over the RPC pipe — §6.2.1)
// ---------------------------------------------------------------------

#[derive(serde::Serialize)]
struct AuthorizeArgs<'a> {
    client_id: &'a str,
    scopes: &'a [&'a str],
    // Undocumented in the RPC AUTHORIZE command reference (its documented
    // args are just `scopes`/`client_id`/`rpc_token`/`username`), but live
    // testing showed it's required in practice: this app's public-client
    // configuration makes Discord's token endpoint demand a matching
    // `code_verifier` on exchange ("Code challenge failed" without one), so
    // the challenge has to be registered somewhere — passing it through
    // here (mirroring the web `/oauth2/authorize` endpoint's query params)
    // is the only place left to put it.
    code_challenge: &'a str,
    code_challenge_method: &'a str,
}

#[derive(serde::Deserialize)]
struct AuthorizeResponseData {
    code: String,
}

/// A random 64-character verifier (two concatenated UUIDv4s, hex — safely
/// within PKCE's 43-128 char unreserved-charset requirement without pulling
/// in a `rand` dependency) plus its SHA-256 `code_challenge`.
fn generate_pkce_pair() -> (String, String) {
    let verifier = format!("{}{}", Uuid::new_v4().as_simple(), Uuid::new_v4().as_simple());
    let digest = sha2::Sha256::digest(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

/// Sends the RPC `AUTHORIZE` command. Discord shows its own in-client
/// consent dialog and returns an authorization code via the RPC response —
/// an `ERROR` frame here (a non-null `code` field) means the app isn't an
/// approved/registered tester (§6.2.5's 50-tester cap). Returns the code
/// together with the PKCE verifier that must accompany it at the token
/// endpoint.
async fn request_authorization(client: &mut DiscordRpcClient, client_id: &str) -> Result<(String, String), AuthError> {
    let (verifier, challenge) = generate_pkce_pair();
    let response = client
        .send_command(
            "AUTHORIZE",
            AuthorizeArgs {
                client_id,
                scopes: SCOPES,
                code_challenge: &challenge,
                code_challenge_method: "S256",
            },
        )
        .await?;

    if let Some(code) = response.code {
        tracing::warn!(
            code,
            message = ?response.message,
            "discord AUTHORIZE returned an ERROR frame (app not approved for RPC, or the tester cap was reached)"
        );
        return Err(AuthError::RpcNotApproved);
    }
    let Some(data) = response.data else {
        // A "successful" (no error code) reply with no `data` at all — e.g.
        // the user dismissed/declined the native consent dialog without an
        // explicit ERROR frame, or Discord silently couldn't grant one of
        // the requested scopes.
        tracing::warn!(
            message = ?response.message,
            "discord AUTHORIZE reply had no `data` field (likely declined/dismissed, or a requested scope could not be granted)"
        );
        return Err(AuthError::NotAuthorized);
    };
    match serde_json::from_value::<AuthorizeResponseData>(data.clone()) {
        Ok(parsed) => Ok((parsed.code, verifier)),
        Err(err) => {
            tracing::warn!(
                error = %err,
                data = %data,
                "discord AUTHORIZE reply had a `data` field but it didn't contain the expected `code` string"
            );
            Err(AuthError::NotAuthorized)
        }
    }
}

// ---------------------------------------------------------------------
// Discord OAuth2 token endpoint client (no client secret)
// ---------------------------------------------------------------------

pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

// Manual (not derived) Debug: redacts the secret fields so an accidental
// `tracing::debug!(?resp)` anywhere can't leak a token (§15).
impl std::fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenResponse")
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

#[derive(serde::Deserialize)]
struct DiscordTokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

#[derive(Clone)]
pub struct DiscordOAuthClient {
    http: reqwest::Client,
    token_url: String,
    revoke_url: String,
}

impl Default for DiscordOAuthClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscordOAuthClient {
    pub fn new() -> Self {
        Self::with_urls(TOKEN_URL, REVOKE_URL)
    }

    fn with_urls(token_url: impl Into<String>, revoke_url: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build reqwest client");
        Self {
            http,
            token_url: token_url.into(),
            revoke_url: revoke_url.into(),
        }
    }

    /// Exchanges a code obtained via the RPC `AUTHORIZE` command for a
    /// token pair. No `client_secret` (public client) and no
    /// `redirect_uri` — this code did not come from a browser redirect, so
    /// neither applies. `code_verifier` must be the one paired with the
    /// `code_challenge` sent in the matching `AUTHORIZE` call (see
    /// [`generate_pkce_pair`]) — live testing showed Discord's token
    /// endpoint enforces the full PKCE match (`invalid_grant: Code
    /// challenge failed.`) even for a code obtained this way, not just the
    /// verifier's presence.
    pub async fn exchange_code(&self, client_id: &str, code: &str, code_verifier: &str) -> Result<TokenResponse, AuthError> {
        self.post_token(&[
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", code_verifier),
        ])
        .await
    }

    pub async fn refresh(&self, client_id: &str, refresh_token: &str) -> Result<TokenResponse, AuthError> {
        self.post_token(&[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .await
    }

    pub async fn revoke(&self, client_id: &str, token: &str) -> Result<(), AuthError> {
        let response = self
            .http
            .post(&self.revoke_url)
            .form(&[("client_id", client_id), ("token", token)])
            .send()
            .await
            .map_err(|_| AuthError::DiscordUnreachable)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(AuthError::DiscordUnreachable)
        }
    }

    async fn post_token(&self, params: &[(&str, &str)]) -> Result<TokenResponse, AuthError> {
        let response = self
            .http
            .post(&self.token_url)
            .form(params)
            .send()
            .await
            .map_err(|_| AuthError::DiscordUnreachable)?;

        if !response.status().is_success() {
            let status = response.status();
            // Discord's error body (`{"error": "...", "error_description":
            // "..."}`) is the only way to tell "this code/refresh token is
            // stale" apart from "we're sending a malformed request" — both
            // currently surface as the same HTTP 400, but only one of them
            // should map to `ReauthorizationRequired`. Log it (not a secret;
            // it's Discord's own generic error text) until that's proven
            // out against a real 400.
            let body = response.text().await.unwrap_or_default();
            tracing::warn!(%status, %body, "discord oauth2 token endpoint returned an error");
            if status == reqwest::StatusCode::BAD_REQUEST {
                return Err(AuthError::ReauthorizationRequired);
            }
            return Err(AuthError::DiscordUnreachable);
        }

        let parsed: DiscordTokenResponse = response.json().await.map_err(|_| AuthError::DiscordUnreachable)?;
        Ok(TokenResponse {
            access_token: parsed.access_token,
            refresh_token: parsed.refresh_token,
            expires_in: parsed.expires_in,
        })
    }
}

fn store_from_response(resp: TokenResponse) -> StoredTokenSet {
    StoredTokenSet {
        access_token: SecretString::from(resp.access_token),
        refresh_token: SecretString::from(resp.refresh_token),
        expires_at: SystemTime::now() + Duration::from_secs(resp.expires_in),
        scopes: SCOPES.iter().map(|s| s.to_string()).collect(),
    }
}

fn clone_token_set(tokens: &StoredTokenSet) -> StoredTokenSet {
    StoredTokenSet {
        access_token: SecretString::from(tokens.access_token.expose_secret().to_string()),
        refresh_token: SecretString::from(tokens.refresh_token.expose_secret().to_string()),
        expires_at: tokens.expires_at,
        scopes: tokens.scopes.clone(),
    }
}

// ---------------------------------------------------------------------
// Startup orchestration (§12.1 "Discord認証")
// ---------------------------------------------------------------------

/// Convenience bundle for the three things `DiscordRpcClient::run()` needs
/// to service an on-demand authorization request.
pub struct AuthHandles<'a> {
    pub store: &'a dyn TokenStore,
    pub oauth: &'a DiscordOAuthClient,
    pub cfg: &'a AuthConfig,
}

/// Silently restores a previously-stored session (authenticating or
/// refreshing as needed) — never shows the RPC `AUTHORIZE` consent dialog.
/// Returns `Ok(None)` when nothing is stored yet; the caller must invoke
/// [`authorize_now`] (wired to an explicit user action, e.g. a Settings
/// button) to obtain one.
pub async fn try_restore_session(
    client: &mut DiscordRpcClient,
    store: &dyn TokenStore,
    oauth: &DiscordOAuthClient,
    cfg: &AuthConfig,
) -> Result<Option<StoredTokenSet>, AuthError> {
    match store.load()? {
        Some(tokens) if !tokens.is_expired_within(Duration::ZERO) => {
            match client.authenticate(tokens.access_token.expose_secret()).await {
                Ok(()) => Ok(Some(tokens)),
                Err(_) => try_refresh(client, store, oauth, cfg, &tokens).await,
            }
        }
        Some(tokens) => try_refresh(client, store, oauth, cfg, &tokens).await,
        None => Ok(None),
    }
}

/// Drives the full `AUTHORIZE` -> exchange -> `AUTHENTICATE` -> store
/// sequence. Shows Discord's native in-client consent dialog — only call
/// this in direct response to an explicit user action.
pub async fn authorize_now(
    client: &mut DiscordRpcClient,
    store: &dyn TokenStore,
    oauth: &DiscordOAuthClient,
    cfg: &AuthConfig,
) -> Result<StoredTokenSet, AuthError> {
    let (code, verifier) = request_authorization(client, &cfg.client_id).await?;
    let token_response = oauth.exchange_code(&cfg.client_id, &code, &verifier).await?;
    let tokens = store_from_response(token_response);
    client.authenticate(tokens.access_token.expose_secret()).await?;
    store.save(&tokens)?;
    Ok(tokens)
}

async fn try_refresh(
    client: &mut DiscordRpcClient,
    store: &dyn TokenStore,
    oauth: &DiscordOAuthClient,
    cfg: &AuthConfig,
    current: &StoredTokenSet,
) -> Result<Option<StoredTokenSet>, AuthError> {
    match oauth.refresh(&cfg.client_id, current.refresh_token.expose_secret()).await {
        Ok(resp) => {
            let tokens = store_from_response(resp);
            client.authenticate(tokens.access_token.expose_secret()).await?;
            store.save(&tokens)?;
            Ok(Some(tokens))
        }
        Err(AuthError::ReauthorizationRequired) => {
            // Don't pop an unsolicited consent dialog — clear the stale
            // token and wait for the user to re-authorize explicitly.
            tracing::warn!("stored discord refresh token is no longer valid; clearing it (re-authorize via Settings)");
            let _ = store.clear();
            Ok(None)
        }
        Err(AuthError::DiscordUnreachable) if !current.is_expired_within(Duration::ZERO) => {
            // §22 "Token Broker停止"-equivalent: keep using the still-valid
            // cached token rather than treating an unreachable endpoint as
            // fatal.
            client.authenticate(current.access_token.expose_secret()).await?;
            Ok(Some(clone_token_set(current)))
        }
        Err(err) => Err(err),
    }
}

// ---------------------------------------------------------------------
// Background refresh (§6.2.4: refresh 5 minutes before expiry)
// ---------------------------------------------------------------------

const REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);
const REFRESH_RETRY_DELAY: Duration = Duration::from_secs(60);

struct RefreshTimer {
    deadline: Option<tokio::time::Instant>,
}

impl RefreshTimer {
    fn new() -> Self {
        Self { deadline: None }
    }

    fn arm(&mut self, expires_at: SystemTime) {
        let refresh_at = expires_at
            .checked_sub(REFRESH_MARGIN)
            .unwrap_or_else(SystemTime::now);
        let delay = refresh_at
            .duration_since(SystemTime::now())
            .unwrap_or(Duration::ZERO);
        self.deadline = Some(tokio::time::Instant::now() + delay);
    }

    fn disarm(&mut self) {
        self.deadline = None;
    }

    async fn tick(&mut self) {
        match self.deadline {
            Some(deadline) => tokio::time::sleep_until(deadline).await,
            None => std::future::pending().await,
        }
    }
}

/// Runs forever, independent of any live RPC connection: refreshing the
/// access token does **not** require re-calling `AUTHENTICATE` on an
/// already-authenticated pipe — it only needs to keep the persisted
/// [`StoredTokenSet`] fresh for the *next* `AUTHENTICATE` (after a
/// reconnect, or app restart).
pub async fn run_refresh_loop(store: std::sync::Arc<dyn TokenStore>, oauth: DiscordOAuthClient, cfg: AuthConfig) {
    let mut timer = RefreshTimer::new();
    loop {
        match store.load() {
            Ok(Some(tokens)) => timer.arm(tokens.expires_at),
            Ok(None) => timer.disarm(),
            Err(err) => {
                tracing::warn!(error = %err, "failed to load stored tokens for refresh scheduling");
                timer.disarm();
            }
        }

        timer.tick().await;

        let current = match store.load() {
            Ok(Some(tokens)) => tokens,
            Ok(None) => continue,
            Err(err) => {
                tracing::warn!(error = %err, "failed to load stored tokens before refresh attempt");
                tokio::time::sleep(REFRESH_RETRY_DELAY).await;
                continue;
            }
        };

        match oauth.refresh(&cfg.client_id, current.refresh_token.expose_secret()).await {
            Ok(resp) => {
                let tokens = store_from_response(resp);
                if let Err(err) = store.save(&tokens) {
                    tracing::warn!(error = %err, "failed to persist refreshed discord token");
                } else {
                    tracing::info!("discord access token refreshed");
                }
            }
            Err(AuthError::ReauthorizationRequired) => {
                tracing::warn!("stored discord refresh token is no longer valid; clearing it (re-authorize via Settings)");
                let _ = store.clear();
            }
            Err(err) => {
                tracing::warn!(error = %err, "scheduled discord token refresh failed; will retry");
                tokio::time::sleep(REFRESH_RETRY_DELAY).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn exchange_code_parses_a_successful_token_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "at-1", "refresh_token": "rt-1", "expires_in": 604800
            })))
            .mount(&server)
            .await;

        let client = DiscordOAuthClient::with_urls(format!("{}/token", server.uri()), format!("{}/revoke", server.uri()));
        let resp = client.exchange_code("cid", "code123", "verifier").await.unwrap();
        assert_eq!(resp.access_token, "at-1");
        assert_eq!(resp.refresh_token, "rt-1");
    }

    #[tokio::test]
    async fn refresh_maps_400_to_reauthorization_required() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&server)
            .await;

        let client = DiscordOAuthClient::with_urls(format!("{}/token", server.uri()), format!("{}/revoke", server.uri()));
        let err = client.refresh("cid", "stale-refresh-token").await.unwrap_err();
        assert!(matches!(err, AuthError::ReauthorizationRequired));
    }

    #[tokio::test]
    async fn refresh_maps_server_error_to_discord_unreachable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let client = DiscordOAuthClient::with_urls(format!("{}/token", server.uri()), format!("{}/revoke", server.uri()));
        let err = client.refresh("cid", "some-refresh-token").await.unwrap_err();
        assert!(matches!(err, AuthError::DiscordUnreachable));
    }

    #[tokio::test]
    async fn revoke_succeeds_on_2xx() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/revoke"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let client = DiscordOAuthClient::with_urls(format!("{}/token", server.uri()), format!("{}/revoke", server.uri()));
        client.revoke("cid", "some-token").await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn refresh_timer_fires_five_minutes_before_expiry() {
        let mut timer = RefreshTimer::new();
        timer.arm(SystemTime::now() + Duration::from_secs(360));

        let almost_fired = tokio::time::timeout(Duration::from_secs(59), timer.tick()).await;
        assert!(almost_fired.is_err(), "must not fire before the refresh margin");

        let fired = tokio::time::timeout(Duration::from_secs(5), timer.tick()).await;
        assert!(fired.is_ok(), "must fire once within the refresh margin");
    }

    #[tokio::test(start_paused = true)]
    async fn refresh_timer_fires_immediately_when_already_within_margin() {
        let mut timer = RefreshTimer::new();
        timer.arm(SystemTime::now() + Duration::from_secs(30));

        let fired = tokio::time::timeout(Duration::from_millis(50), timer.tick()).await;
        assert!(fired.is_ok(), "must fire immediately when already within the refresh margin");
    }

    #[tokio::test]
    async fn disarmed_timer_never_fires() {
        let mut timer = RefreshTimer::new();
        timer.disarm();
        let fired = tokio::time::timeout(Duration::from_millis(50), timer.tick()).await;
        assert!(fired.is_err(), "a disarmed timer must never fire");
    }
}
