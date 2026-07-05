# Admin OAuth Login Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Admin UI OAuth account login for Google, Github, BuilderId, and Enterprise by letting users paste the provider callback URL and by converting the token exchange result into existing `KiroCredentials`.

**Architecture:** Add a focused backend OAuth module under `src/admin/oauth.rs` for provider metadata, PKCE, callback parsing, token exchange, session state, and credential mapping. Wire it into `AdminService`, `src/admin/handlers.rs`, and `src/admin/router.rs` with authenticated `/api/admin/oauth/*` endpoints. Add a frontend OAuth dialog that starts a session, opens/copies the auth URL, accepts a pasted callback URL, completes the session, and refreshes the credential list.

**Tech Stack:** Rust 2024, Axum 0.8, reqwest 0.12, parking_lot, serde, chrono, sha2, base64, urlencoding, React 19, TypeScript, TanStack Query, Axios, Radix dialog, lucide-react.

## Global Constraints

- Social providers (`Google`, `Github`) use `kiro://kiro.kiroAgent/authenticate-success`; do not use Social HTTP callbacks.
- IdC providers (`BuilderId`, `Enterprise`) use AWS SSO OIDC and pasted callback URL completion for the first implementation.
- All `/api/admin/oauth/*` endpoints require the existing Admin API key.
- Users paste a full callback URL; the app parses `code` and `state`.
- `state` is generated server-side and verified on completion.
- `codeVerifier` remains server-side only.
- Tokens, authorization codes, callback URLs, and client secrets are not logged or returned to the frontend.
- Completed sessions clear sensitive fields immediately and retain only sanitized result metadata until TTL.
- Manual add/import credential workflows remain unchanged.
- Existing token manager validation, refresh, usage lookup, model availability, and persistence paths are reused.
- Do not implement a public OAuth callback service.
- Do not require DNS, HTTPS, or reverse proxy configuration.
- This workspace has no Rust toolchain on the host and this crate has no `lib` target; backend verification commands use `docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test ...` without `--lib`.

---

## File Structure

- `Cargo.toml`: add `getrandom = "0.2"` and `url = "2"` for OS randomness and robust query parsing.
- `src/admin/oauth.rs`: new backend OAuth module. Owns provider enum, request/response structs, session store, PKCE helpers, callback parser, Social/IdC URL builders, token exchange clients, credential mapping, and unit tests.
- `src/admin/mod.rs`: expose the `oauth` module.
- `src/kiro/token_manager.rs`: add a read-only `global_proxy()` accessor so OAuth token exchange reuses the same runtime proxy as credential refresh.
- `src/admin/service.rs`: add `oauth_sessions: OAuthSessionStore`; add start/status/cancel/complete methods; call `MultiTokenManager::add_credential` with mapped `KiroCredentials`; refresh usage after creation.
- `src/admin/handlers.rs`: add OAuth handlers that delegate to `AdminService`.
- `src/admin/router.rs`: add authenticated `/oauth/start`, `/oauth/complete`, `/oauth/status/{session_id}`, and `/oauth/cancel/{session_id}` routes.
- `admin-ui/src/types/api.ts`: add OAuth request/response types.
- `admin-ui/src/api/credentials.ts`: add OAuth API functions using the existing authenticated Axios instance.
- `admin-ui/src/hooks/use-credentials.ts`: add OAuth mutations/status query helpers.
- `admin-ui/src/components/oauth-login-dialog.tsx`: new OAuth login dialog.
- `admin-ui/src/components/dashboard.tsx`: wire the dialog into the toolbar next to add/import credential actions.

---

### Task 1: Backend OAuth Core Helpers

**Files:**
- Modify: `Cargo.toml`
- Create: `src/admin/oauth.rs`
- Modify: `src/admin/mod.rs`

**Interfaces:**
- Produces: `OAuthProvider`, `AuthMethod`, `OAuthStartRequest`, `OAuthStartResponse`, `OAuthCompleteRequest`, `OAuthCompleteResponse`, `OAuthStatusResponse`, `OAuthSessionStore`, `ParsedCallback`, `generate_pkce_pair()`, `parse_callback_input()`, `build_social_auth_url()`, `build_idc_authorize_url()`.
- Consumes: existing `KiroCredentials`, `Config`, `ProxyConfig` in later tasks.

- [ ] **Step 1: Add dependencies**

Modify `Cargo.toml` dependencies:

```toml
getrandom = "0.2"    # OS randomness for OAuth state and PKCE verifier
url = "2"            # URL/query parsing for pasted OAuth callback URLs
```

- [ ] **Step 2: Create failing unit tests for PKCE and callback parsing**

Create `src/admin/oauth.rs` with the module skeleton and these tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_pair_is_url_safe_and_non_empty() {
        let pair = generate_pkce_pair().expect("pkce should generate");
        assert!(pair.verifier.len() >= 40);
        assert!(pair.challenge.len() >= 40);
        assert!(!pair.verifier.contains('='));
        assert!(!pair.challenge.contains('='));
        assert!(pair.verifier.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')
        }));
    }

    #[test]
    fn callback_parser_reads_social_deep_link() {
        let parsed = parse_callback_input(
            "kiro://kiro.kiroAgent/authenticate-success?code=abc%2F123&state=state-1",
        )
        .expect("callback should parse");
        assert_eq!(parsed.code, "abc/123");
        assert_eq!(parsed.state, "state-1");
    }

    #[test]
    fn callback_parser_reads_loopback_url() {
        let parsed = parse_callback_input(
            "http://127.0.0.1:49152/oauth/callback?code=idc-code&state=idc-state",
        )
        .expect("callback should parse");
        assert_eq!(parsed.code, "idc-code");
        assert_eq!(parsed.state, "idc-state");
    }

    #[test]
    fn callback_parser_rejects_missing_code() {
        let err = parse_callback_input(
            "kiro://kiro.kiroAgent/authenticate-success?state=state-1",
        )
        .unwrap_err();
        assert!(err.to_string().contains("回调 URL 缺少 code"));
    }

    #[test]
    fn social_auth_url_uses_kiro_deep_link_redirect() {
        let url = build_social_auth_url(
            OAuthProvider::Google,
            "challenge",
            "state",
        );
        assert!(url.contains("idp=Google"));
        assert!(url.contains("redirect_uri=kiro%3A%2F%2Fkiro.kiroAgent%2Fauthenticate-success"));
        assert!(url.contains("code_challenge=challenge"));
        assert!(url.contains("state=state"));
    }
}
```

Run:

```bash
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test admin::oauth
```

Expected: FAIL because the module functions and types do not exist.

- [ ] **Step 3: Implement core types and pure helpers**

Add this implementation above the tests in `src/admin/oauth.rs`:

```rust
use std::collections::HashMap;
use anyhow::{Context, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SOCIAL_REDIRECT_URI: &str = "kiro://kiro.kiroAgent/authenticate-success";
pub const BUILDER_ID_START_URL: &str = "https://view.awsapps.com/start";
pub const IDC_REGISTER_REDIRECT_URI: &str = "http://127.0.0.1/oauth/callback";
pub const IDC_CALLBACK_PORT: u16 = 49152;
pub const OAUTH_SESSION_TTL_SECS: i64 = 600;

pub const IDC_SCOPES: &[&str] = &[
    "codewhisperer:completions",
    "codewhisperer:analysis",
    "codewhisperer:conversations",
    "codewhisperer:transformations",
    "codewhisperer:taskassist",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthMethod {
    Social,
    Idc,
}

impl AuthMethod {
    pub fn as_credential_value(self) -> &'static str {
        match self {
            AuthMethod::Social => "social",
            AuthMethod::Idc => "idc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OAuthProvider {
    Google,
    Github,
    BuilderId,
    Enterprise,
}

impl OAuthProvider {
    pub fn auth_method(self) -> AuthMethod {
        match self {
            OAuthProvider::Google | OAuthProvider::Github => AuthMethod::Social,
            OAuthProvider::BuilderId | OAuthProvider::Enterprise => AuthMethod::Idc,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OAuthProvider::Google => "Google",
            OAuthProvider::Github => "Github",
            OAuthProvider::BuilderId => "BuilderId",
            OAuthProvider::Enterprise => "Enterprise",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStartRequest {
    pub provider: OAuthProvider,
    pub region: Option<String>,
    pub start_url: Option<String>,
    #[serde(default)]
    pub priority: u32,
    pub endpoint: Option<String>,
    pub proxy_url: Option<String>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStartResponse {
    pub session_id: String,
    pub provider: OAuthProvider,
    pub auth_method: AuthMethod,
    pub auth_url: String,
    pub redirect_uri: String,
    pub expires_at: String,
    pub completion_mode: &'static str,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthCompleteRequest {
    pub session_id: String,
    pub callback_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthCompleteResponse {
    pub success: bool,
    pub credential_id: u64,
    pub email: Option<String>,
    pub subscription_title: Option<String>,
    pub supported_model_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OAuthSessionState {
    Pending,
    Completed,
    Failed,
    Cancelled,
    Expired,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStatusResponse {
    pub session_id: String,
    pub state: OAuthSessionState,
    pub provider: OAuthProvider,
    pub expires_at: String,
    pub credential_id: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCallback {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Clone)]
pub struct PkcePair {
    pub verifier: String,
    pub challenge: String,
}

#[derive(Debug, Clone)]
pub struct OAuthSession {
    pub session_id: String,
    pub provider: OAuthProvider,
    pub auth_method: AuthMethod,
    pub state: String,
    pub code_verifier: Option<String>,
    pub redirect_uri: String,
    pub region: String,
    pub start_url: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub machine_id: String,
    pub priority: u32,
    pub endpoint: Option<String>,
    pub proxy_url: Option<String>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub state_kind: OAuthSessionState,
    pub credential_id: Option<u64>,
    pub error: Option<String>,
}

impl OAuthSession {
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.expires_at
    }

    pub fn scrub_terminal_secrets(&mut self) {
        if matches!(
            self.state_kind,
            OAuthSessionState::Completed
                | OAuthSessionState::Failed
                | OAuthSessionState::Cancelled
                | OAuthSessionState::Expired
        ) {
            self.state.clear();
            self.code_verifier = None;
            self.redirect_uri.clear();
            self.start_url = None;
            self.client_secret = None;
            self.proxy_password = None;
        }
    }

    pub fn sanitized_status(&self, now: DateTime<Utc>) -> OAuthStatusResponse {
        let state = if self.is_expired(now) && self.state_kind == OAuthSessionState::Pending {
            OAuthSessionState::Expired
        } else {
            self.state_kind
        };

        OAuthStatusResponse {
            session_id: self.session_id.clone(),
            state,
            provider: self.provider,
            expires_at: self.expires_at.to_rfc3339(),
            credential_id: self.credential_id,
            error: self.error.clone(),
        }
    }
}

#[derive(Debug, Default)]
pub struct OAuthSessionStore {
    sessions: Mutex<HashMap<String, OAuthSession>>,
}

impl OAuthSessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, mut session: OAuthSession) {
        self.prune_expired();
        session.scrub_terminal_secrets();
        self.sessions
            .lock()
            .insert(session.session_id.clone(), session);
    }

    pub fn get(&self, session_id: &str) -> Option<OAuthSession> {
        self.prune_expired();
        self.sessions.lock().get(session_id).cloned()
    }

    pub fn update(&self, mut session: OAuthSession) {
        session.scrub_terminal_secrets();
        self.sessions
            .lock()
            .insert(session.session_id.clone(), session);
    }

    pub fn remove(&self, session_id: &str) -> Option<OAuthSession> {
        self.sessions.lock().remove(session_id)
    }

    fn prune_expired(&self) {
        let now = Utc::now();
        self.sessions.lock().retain(|_, session| {
            !(session.is_expired(now)
                && matches!(
                    session.state_kind,
                    OAuthSessionState::Pending
                        | OAuthSessionState::Failed
                        | OAuthSessionState::Cancelled
                ))
        });
    }
}

fn random_bytes<const N: usize>() -> anyhow::Result<[u8; N]> {
    let mut bytes = [0_u8; N];
    getrandom::getrandom(&mut bytes).context("OS randomness unavailable")?;
    Ok(bytes)
}

fn random_urlsafe<const N: usize>() -> anyhow::Result<String> {
    Ok(URL_SAFE_NO_PAD.encode(random_bytes::<N>()?))
}

pub fn generate_pkce_pair() -> anyhow::Result<PkcePair> {
    let verifier = random_urlsafe::<32>()?;
    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(digest);
    Ok(PkcePair {
        verifier,
        challenge,
    })
}

pub fn generate_session_id() -> anyhow::Result<String> {
    random_urlsafe::<24>()
}

pub fn generate_state() -> anyhow::Result<String> {
    random_urlsafe::<24>()
}

pub fn generate_machine_id() -> anyhow::Result<String> {
    Ok(hex::encode(random_bytes::<32>()?))
}

pub fn session_expiry(created_at: DateTime<Utc>) -> DateTime<Utc> {
    created_at + Duration::seconds(OAUTH_SESSION_TTL_SECS)
}

pub fn parse_callback_input(input: &str) -> anyhow::Result<ParsedCallback> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("回调 URL 为空");
    }

    let query = if let Ok(url) = url::Url::parse(trimmed) {
        url.query().unwrap_or("").to_string()
    } else {
        trimmed
            .split_once('?')
            .map(|(_, query)| query.to_string())
            .unwrap_or_else(|| trimmed.to_string())
    };

    let params: HashMap<String, String> =
        url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();

    if let Some(error) = params.get("error") {
        let desc = params
            .get("error_description")
            .map(String::as_str)
            .unwrap_or("未知错误");
        bail!("OAuth provider returned error: {}: {}", error, desc);
    }

    let code = params
        .get("code")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("回调 URL 缺少 code"))?;
    let state = params
        .get("state")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("回调 URL 缺少 state"))?;

    Ok(ParsedCallback { code, state })
}

pub fn build_social_auth_url(
    provider: OAuthProvider,
    code_challenge: &str,
    state: &str,
) -> String {
    format!(
        "https://prod.us-east-1.auth.desktop.kiro.dev/login?idp={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}",
        provider.as_str(),
        urlencoding::encode(SOCIAL_REDIRECT_URI),
        urlencoding::encode(code_challenge),
        urlencoding::encode(state)
    )
}

pub fn idc_callback_redirect_uri() -> String {
    format!("http://127.0.0.1:{}/oauth/callback", IDC_CALLBACK_PORT)
}

pub fn build_idc_authorize_url(
    region: &str,
    client_id: &str,
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
) -> String {
    format!(
        "https://oidc.{}.amazonaws.com/authorize?response_type=code&client_id={}&redirect_uri={}&scopes={}&state={}&code_challenge={}&code_challenge_method=S256",
        region,
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(&IDC_SCOPES.join(",")),
        urlencoding::encode(state),
        urlencoding::encode(code_challenge)
    )
}

```

Modify `src/admin/mod.rs`:

```rust
pub mod oauth;
```

- [ ] **Step 4: Run backend tests for Task 1**

Run:

```bash
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test admin::oauth
```

Expected: PASS for the PKCE, callback parser, and URL-builder tests.

- [ ] **Step 5: Commit Task 1**

```bash
git add Cargo.toml Cargo.lock src/admin/mod.rs src/admin/oauth.rs
git commit -m "feat: add admin oauth core helpers"
```

---

### Task 2: OAuth Token Exchange and Credential Mapping

**Files:**
- Modify: `src/admin/oauth.rs`

**Interfaces:**
- Consumes: Task 1 `OAuthSession`, provider helpers, `KiroCredentials`, `Config`, `ProxyConfig`.
- Produces: `exchange_social_token()`, `register_idc_client()`, `exchange_idc_token()`, `map_social_credentials()`, `map_idc_credentials()`.

- [ ] **Step 1: Write failing tests for credential mapping**

Append tests in `src/admin/oauth.rs`:

```rust
#[test]
fn social_mapper_preserves_provider_identity() {
    let session = OAuthSession {
        session_id: "s1".to_string(),
        provider: OAuthProvider::Github,
        auth_method: AuthMethod::Social,
        state: "state".to_string(),
        code_verifier: Some("verifier".to_string()),
        redirect_uri: SOCIAL_REDIRECT_URI.to_string(),
        region: "us-east-1".to_string(),
        start_url: None,
        client_id: None,
        client_secret: None,
        machine_id: "machine-1".to_string(),
        priority: 7,
        endpoint: Some("ide".to_string()),
        proxy_url: Some("direct".to_string()),
        proxy_username: None,
        proxy_password: None,
        created_at: Utc::now(),
        expires_at: session_expiry(Utc::now()),
        state_kind: OAuthSessionState::Pending,
        credential_id: None,
        error: None,
    };
    let token = SocialTokenResponse {
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        profile_arn: Some("arn:aws:kiro:profile".to_string()),
        expires_in: Some(3600),
    };

    let cred = map_social_credentials(&session, token);
    assert_eq!(cred.auth_method.as_deref(), Some("social"));
    assert_eq!(cred.access_token.as_deref(), Some("access"));
    assert_eq!(cred.refresh_token.as_deref(), Some("refresh"));
    assert_eq!(cred.profile_arn.as_deref(), Some("arn:aws:kiro:profile"));
    assert_eq!(cred.machine_id.as_deref(), Some("machine-1"));
    assert_eq!(cred.idp.as_deref(), Some("Github"));
    assert_eq!(cred.priority, 7);
    assert_eq!(cred.endpoint.as_deref(), Some("ide"));
    assert_eq!(cred.proxy_url.as_deref(), Some("direct"));
    assert!(cred.expires_at.is_some());
}

#[test]
fn idc_mapper_sets_refresh_fields() {
    let session = OAuthSession {
        session_id: "s1".to_string(),
        provider: OAuthProvider::BuilderId,
        auth_method: AuthMethod::Idc,
        state: "state".to_string(),
        code_verifier: Some("verifier".to_string()),
        redirect_uri: idc_callback_redirect_uri(),
        region: "us-west-2".to_string(),
        start_url: Some(BUILDER_ID_START_URL.to_string()),
        client_id: Some("client-id".to_string()),
        client_secret: Some("client-secret".to_string()),
        machine_id: "machine-2".to_string(),
        priority: 3,
        endpoint: None,
        proxy_url: None,
        proxy_username: None,
        proxy_password: None,
        created_at: Utc::now(),
        expires_at: session_expiry(Utc::now()),
        state_kind: OAuthSessionState::Pending,
        credential_id: None,
        error: None,
    };
    let token = IdcTokenResponse {
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        expires_in: Some(7200),
        id_token: Some("id-token".to_string()),
        token_type: Some("Bearer".to_string()),
    };

    let cred = map_idc_credentials(&session, token).expect("credential maps");
    assert_eq!(cred.auth_method.as_deref(), Some("idc"));
    assert_eq!(cred.access_token.as_deref(), Some("access"));
    assert_eq!(cred.refresh_token.as_deref(), Some("refresh"));
    assert_eq!(cred.client_id.as_deref(), Some("client-id"));
    assert_eq!(cred.client_secret.as_deref(), Some("client-secret"));
    assert_eq!(cred.region.as_deref(), Some("us-west-2"));
    assert_eq!(cred.machine_id.as_deref(), Some("machine-2"));
    assert!(cred.profile_arn.is_none());
    assert!(cred.expires_at.is_some());
}
```

Run:

```bash
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test social_mapper_preserves_provider_identity
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test idc_mapper_sets_refresh_fields
```

Expected: FAIL because exchange response structs and mapper functions do not exist.

- [ ] **Step 2: Add HTTP response structs and mapping functions**

Add to `src/admin/oauth.rs`:

```rust
use crate::http_client::{ProxyConfig, build_client};
use crate::kiro::model::credentials::KiroCredentials;
use crate::model::config::Config;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialTokenResponse {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "refreshToken")]
    pub refresh_token: String,
    #[serde(rename = "profileArn")]
    pub profile_arn: Option<String>,
    #[serde(rename = "expiresIn")]
    pub expires_in: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdcClientRegistration {
    #[serde(rename = "clientId")]
    pub client_id: String,
    #[serde(rename = "clientSecret")]
    pub client_secret: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdcTokenResponse {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "refreshToken")]
    pub refresh_token: String,
    #[serde(rename = "expiresIn")]
    pub expires_in: Option<i64>,
    #[serde(rename = "idToken")]
    pub id_token: Option<String>,
    #[serde(rename = "tokenType")]
    pub token_type: Option<String>,
}

fn expires_at_from_now(expires_in: Option<i64>) -> Option<String> {
    expires_in.map(|seconds| (Utc::now() + Duration::seconds(seconds)).to_rfc3339())
}

pub fn map_social_credentials(
    session: &OAuthSession,
    token: SocialTokenResponse,
) -> KiroCredentials {
    KiroCredentials {
        id: None,
        access_token: Some(token.access_token),
        refresh_token: Some(token.refresh_token),
        kiro_api_key: None,
        profile_arn: token.profile_arn,
        expires_at: expires_at_from_now(token.expires_in),
        auth_method: Some("social".to_string()),
        client_id: None,
        client_secret: None,
        priority: session.priority,
        region: Some(session.region.clone()),
        api_region: None,
        machine_id: Some(session.machine_id.clone()),
        email: None,
        subscription_title: None,
        proxy_url: session.proxy_url.clone(),
        proxy_username: session.proxy_username.clone(),
        proxy_password: session.proxy_password.clone(),
        endpoint: session.endpoint.clone(),
        idp: Some(session.provider.as_str().to_string()),
        overage_enabled: None,
        disabled: false,
        runtime_only: false,
    }
}

pub fn map_idc_credentials(
    session: &OAuthSession,
    token: IdcTokenResponse,
) -> anyhow::Result<KiroCredentials> {
    let client_id = session
        .client_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("IdC session missing clientId"))?;
    let client_secret = session
        .client_secret
        .clone()
        .ok_or_else(|| anyhow::anyhow!("IdC session missing clientSecret"))?;

    Ok(KiroCredentials {
        id: None,
        access_token: Some(token.access_token),
        refresh_token: Some(token.refresh_token),
        kiro_api_key: None,
        profile_arn: None,
        expires_at: expires_at_from_now(token.expires_in),
        auth_method: Some("idc".to_string()),
        client_id: Some(client_id),
        client_secret: Some(client_secret),
        priority: session.priority,
        region: Some(session.region.clone()),
        api_region: None,
        machine_id: Some(session.machine_id.clone()),
        email: None,
        subscription_title: None,
        proxy_url: session.proxy_url.clone(),
        proxy_username: session.proxy_username.clone(),
        proxy_password: session.proxy_password.clone(),
        endpoint: session.endpoint.clone(),
        idp: None,
        overage_enabled: None,
        disabled: false,
        runtime_only: false,
    })
}
```

- [ ] **Step 3: Add Social token exchange**

Add to `src/admin/oauth.rs`:

```rust
pub async fn exchange_social_token(
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
    machine_id: &str,
    config: &Config,
    proxy: Option<&ProxyConfig>,
) -> anyhow::Result<SocialTokenResponse> {
    #[derive(Serialize)]
    struct Body<'a> {
        code: &'a str,
        code_verifier: &'a str,
        redirect_uri: &'a str,
    }

    let client = build_client(proxy, 60, config.tls_backend)?;
    let user_agent = format!("KiroIDE-{}-{}", config.kiro_version, machine_id);
    let resp = client
        .post("https://prod.us-east-1.auth.desktop.kiro.dev/oauth/token")
        .header("Accept", "application/json, text/plain, */*")
        .header("Content-Type", "application/json")
        .header("User-Agent", user_agent)
        .json(&Body {
            code,
            code_verifier,
            redirect_uri,
        })
        .send()
        .await
        .context("Social token exchange request failed")?;

    let status = resp.status();
    if !status.is_success() {
        bail!("Token 交换失败，请重新授权 (HTTP {})", status.as_u16());
    }

    let text = resp.text().await.unwrap_or_default();
    serde_json::from_str(&text).context("Social token exchange response parse failed")
}
```

- [ ] **Step 4: Add IdC client registration and token exchange**

Add to `src/admin/oauth.rs`:

```rust
pub async fn register_idc_client(
    region: &str,
    start_url: &str,
    config: &Config,
    proxy: Option<&ProxyConfig>,
) -> anyhow::Result<IdcClientRegistration> {
    let client = build_client(proxy, 60, config.tls_backend)?;
    let url = format!("https://oidc.{}.amazonaws.com/client/register", region);
    let scopes: Vec<String> = IDC_SCOPES.iter().map(|scope| (*scope).to_string()).collect();
    let body = serde_json::json!({
        "clientName": "Kiro IDE",
        "clientType": "public",
        "scopes": scopes,
        "grantTypes": ["authorization_code", "refresh_token"],
        "redirectUris": [IDC_REGISTER_REDIRECT_URI],
        "issuerUrl": start_url
    });

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("AWS SSO client registration request failed")?;

    let status = resp.status();
    if !status.is_success() {
        bail!(
            "AWS SSO client 注册失败，请检查 region/startUrl (HTTP {})",
            status.as_u16()
        );
    }

    let text = resp.text().await.unwrap_or_default();
    serde_json::from_str(&text).context("AWS SSO client registration parse failed")
}

pub async fn exchange_idc_token(
    region: &str,
    client_id: &str,
    client_secret: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
    config: &Config,
    proxy: Option<&ProxyConfig>,
) -> anyhow::Result<IdcTokenResponse> {
    let client = build_client(proxy, 60, config.tls_backend)?;
    let url = format!("https://oidc.{}.amazonaws.com/token", region);
    let body = serde_json::json!({
        "clientId": client_id,
        "clientSecret": client_secret,
        "grantType": "authorization_code",
        "code": code,
        "codeVerifier": code_verifier,
        "redirectUri": redirect_uri
    });

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("AWS SSO token exchange request failed")?;

    let status = resp.status();
    if !status.is_success() {
        bail!("Token 交换失败，请重新授权 (HTTP {})", status.as_u16());
    }

    let text = resp.text().await.unwrap_or_default();
    serde_json::from_str(&text).context("AWS SSO token exchange parse failed")
}
```

- [ ] **Step 5: Run backend tests for Task 2**

Run:

```bash
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test admin::oauth
```

Expected: PASS for pure OAuth tests. No external OAuth request is made by these unit tests.

- [ ] **Step 6: Commit Task 2**

```bash
git add src/admin/oauth.rs
git commit -m "feat: map oauth tokens to credentials"
```

---

### Task 3: Admin Service OAuth Session Methods

**Files:**
- Modify: `src/kiro/token_manager.rs`
- Modify: `src/admin/service.rs`

**Interfaces:**
- Consumes: Task 1-2 OAuth store, start/complete requests, exchange functions, credential mappers.
- Produces: `MultiTokenManager::global_proxy`, `AdminService::start_oauth_login`, `AdminService::complete_oauth_login`, `AdminService::oauth_status`, `AdminService::cancel_oauth_login`.

- [ ] **Step 1: Write failing service tests**

Add tests in `src/admin/service.rs` under the existing `#[cfg(test)] mod tests`:

```rust
#[tokio::test]
async fn oauth_start_rejects_enterprise_without_start_url() {
    let service = create_test_service();
    let result = service.start_oauth_login(
        crate::admin::oauth::OAuthStartRequest {
            provider: crate::admin::oauth::OAuthProvider::Enterprise,
            region: Some("us-east-1".to_string()),
            start_url: None,
            priority: 0,
            endpoint: None,
            proxy_url: None,
            proxy_username: None,
            proxy_password: None,
        },
    ).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Enterprise 需要填写 Start URL")
    );
}

#[tokio::test]
async fn oauth_start_social_creates_paste_session() {
    let service = create_test_service();
    let response = service.start_oauth_login(
        crate::admin::oauth::OAuthStartRequest {
            provider: crate::admin::oauth::OAuthProvider::Google,
            region: Some("us-east-1".to_string()),
            start_url: None,
            priority: 0,
            endpoint: Some("ide".to_string()),
            proxy_url: None,
            proxy_username: None,
            proxy_password: None,
        },
    ).await
    .expect("start should succeed");
    assert_eq!(response.provider, crate::admin::oauth::OAuthProvider::Google);
    assert_eq!(response.completion_mode, "pasteCallbackUrl");
    assert_eq!(response.redirect_uri, crate::admin::oauth::SOCIAL_REDIRECT_URI);

    let status = service
        .oauth_status(&response.session_id)
        .expect("status should exist");
    assert_eq!(status.state, crate::admin::oauth::OAuthSessionState::Pending);
}

#[tokio::test]
async fn oauth_cancel_removes_session() {
    let service = create_test_service();
    let response = service.start_oauth_login(
        crate::admin::oauth::OAuthStartRequest {
            provider: crate::admin::oauth::OAuthProvider::Github,
            region: Some("us-east-1".to_string()),
            start_url: None,
            priority: 0,
            endpoint: None,
            proxy_url: None,
            proxy_username: None,
            proxy_password: None,
        },
    ).await
    .expect("start should succeed");

    service
        .cancel_oauth_login(&response.session_id)
        .expect("cancel should succeed");
    assert!(service.oauth_status(&response.session_id).is_err());
}

#[tokio::test]
async fn oauth_complete_wrong_state_records_failed_status() {
    let service = create_test_service();
    let response = service.start_oauth_login(
        crate::admin::oauth::OAuthStartRequest {
            provider: crate::admin::oauth::OAuthProvider::Google,
            region: Some("us-east-1".to_string()),
            start_url: None,
            priority: 0,
            endpoint: None,
            proxy_url: None,
            proxy_username: None,
            proxy_password: None,
        },
    ).await
    .expect("start should succeed");

    let result = service.complete_oauth_login(crate::admin::oauth::OAuthCompleteRequest {
        session_id: response.session_id.clone(),
        callback_url: Some(
            "kiro://kiro.kiroAgent/authenticate-success?code=abc&state=wrong".to_string(),
        ),
    }).await;

    assert!(result.is_err());
    let status = service
        .oauth_status(&response.session_id)
        .expect("failed session should remain visible");
    assert_eq!(status.state, crate::admin::oauth::OAuthSessionState::Failed);
    assert!(status.error.unwrap_or_default().contains("state 不匹配"));
}

#[tokio::test]
async fn oauth_complete_requires_callback_url() {
    let service = create_test_service();
    let response = service.start_oauth_login(
        crate::admin::oauth::OAuthStartRequest {
            provider: crate::admin::oauth::OAuthProvider::Google,
            region: Some("us-east-1".to_string()),
            start_url: None,
            priority: 0,
            endpoint: None,
            proxy_url: None,
            proxy_username: None,
            proxy_password: None,
        },
    ).await
    .expect("start should succeed");

    let result = service.complete_oauth_login(crate::admin::oauth::OAuthCompleteRequest {
        session_id: response.session_id.clone(),
        callback_url: None,
    }).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("callback URL"));
    let status = service
        .oauth_status(&response.session_id)
        .expect("failed session should remain visible");
    assert_eq!(status.state, crate::admin::oauth::OAuthSessionState::Failed);
}
```

Run:

```bash
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test oauth_start
```

Expected: FAIL because the service methods and test helper changes do not exist.

- [ ] **Step 2: Add global proxy getter**

Add this method next to `MultiTokenManager::update_proxy` in `src/kiro/token_manager.rs`:

```rust
/// 获取当前全局代理配置的克隆
pub fn global_proxy(&self) -> Option<ProxyConfig> {
    self.proxy.read().clone()
}
```

- [ ] **Step 3: Add OAuth session store field**

Modify imports in `src/admin/service.rs`:

```rust
use super::oauth::{
    AuthMethod, OAuthCompleteRequest, OAuthCompleteResponse, OAuthProvider, OAuthSession,
    OAuthSessionState, OAuthSessionStore, OAuthStartRequest, OAuthStartResponse,
    OAuthStatusResponse, BUILDER_ID_START_URL, SOCIAL_REDIRECT_URI, build_idc_authorize_url,
    build_social_auth_url, exchange_idc_token, exchange_social_token, generate_machine_id,
    generate_pkce_pair, generate_session_id, generate_state, idc_callback_redirect_uri,
    map_idc_credentials, map_social_credentials, parse_callback_input, register_idc_client,
    session_expiry,
};
```

Add field:

```rust
oauth_sessions: OAuthSessionStore,
```

Initialize in `AdminService::new`:

```rust
oauth_sessions: OAuthSessionStore::new(),
```

- [ ] **Step 4: Add validation helpers**

Add private helpers in `impl AdminService`:

```rust
fn normalize_oauth_region(region: Option<String>) -> String {
    region
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "us-east-1".to_string())
}

fn normalize_oauth_start_url(
    provider: OAuthProvider,
    start_url: Option<String>,
) -> Result<Option<String>, AdminServiceError> {
    match provider {
        OAuthProvider::BuilderId => Ok(Some(BUILDER_ID_START_URL.to_string())),
        OAuthProvider::Enterprise => {
            let value = start_url
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    AdminServiceError::InvalidCredential(
                        "Enterprise 需要填写 Start URL".to_string(),
                    )
                })?;
            if !value.starts_with("https://") {
                return Err(AdminServiceError::InvalidCredential(
                    "Enterprise Start URL 必须以 https:// 开头".to_string(),
                ));
            }
            Ok(Some(value))
        }
        OAuthProvider::Google | OAuthProvider::Github => Ok(None),
    }
}

fn validate_oauth_endpoint(&self, endpoint: Option<String>) -> Result<Option<String>, AdminServiceError> {
    let endpoint = endpoint
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(name) = endpoint.as_deref()
        && !self.known_endpoints.contains(name)
    {
        let mut known: Vec<&str> = self.known_endpoints.iter().map(|s| s.as_str()).collect();
        known.sort_unstable();
        return Err(AdminServiceError::InvalidCredential(format!(
            "endpoint 必须是已注册值，已注册: {:?}，收到: {}",
            known, name
        )));
    }
    Ok(endpoint)
}
```

- [ ] **Step 5: Implement start/status/cancel**

Add public methods in `impl AdminService`:

```rust
pub async fn start_oauth_login(
    &self,
    req: OAuthStartRequest,
) -> Result<OAuthStartResponse, AdminServiceError> {
    let endpoint = self.validate_oauth_endpoint(req.endpoint)?;
    let region = Self::normalize_oauth_region(req.region);
    let start_url = Self::normalize_oauth_start_url(req.provider, req.start_url)?;
    let now = Utc::now();
    let expires_at = session_expiry(now);
    let pkce = generate_pkce_pair().map_err(|e| {
        AdminServiceError::InvalidCredential(format!("OAuth PKCE 生成失败: {}", e))
    })?;
    let state = generate_state().map_err(|e| {
        AdminServiceError::InvalidCredential(format!("OAuth state 生成失败: {}", e))
    })?;
    let session_id = generate_session_id().map_err(|e| {
        AdminServiceError::InvalidCredential(format!("OAuth session 生成失败: {}", e))
    })?;
    let machine_id = generate_machine_id().map_err(|e| {
        AdminServiceError::InvalidCredential(format!("OAuth machineId 生成失败: {}", e))
    })?;

    let proxy = self.token_manager.global_proxy();
    let config = self.config.read().clone();

    let (redirect_uri, auth_url, client_id, client_secret) = match req.provider.auth_method() {
        AuthMethod::Social => {
            let auth_url = build_social_auth_url(req.provider, &pkce.challenge, &state);
            (SOCIAL_REDIRECT_URI.to_string(), auth_url, None, None)
        }
        AuthMethod::Idc => {
            let start_url = start_url.as_deref().ok_or_else(|| {
                AdminServiceError::InvalidCredential("IdC 缺少 Start URL".to_string())
            })?;
            let registration = register_idc_client(&region, start_url, &config, proxy.as_ref())
                .await
                .map_err(|e| self.classify_add_error(e))?;
            let redirect_uri = idc_callback_redirect_uri();
            let auth_url = build_idc_authorize_url(
                &region,
                &registration.client_id,
                &redirect_uri,
                &pkce.challenge,
                &state,
            );
            (
                redirect_uri,
                auth_url,
                Some(registration.client_id),
                Some(registration.client_secret),
            )
        }
    };

    let session = OAuthSession {
        session_id: session_id.clone(),
        provider: req.provider,
        auth_method: req.provider.auth_method(),
        state,
        code_verifier: Some(pkce.verifier),
        redirect_uri: redirect_uri.clone(),
        region,
        start_url,
        client_id,
        client_secret,
        machine_id,
        priority: req.priority,
        endpoint,
        proxy_url: req.proxy_url,
        proxy_username: req.proxy_username,
        proxy_password: req.proxy_password,
        created_at: now,
        expires_at,
        state_kind: OAuthSessionState::Pending,
        credential_id: None,
        error: None,
    };
    self.oauth_sessions.insert(session);

    Ok(OAuthStartResponse {
        session_id,
        provider: req.provider,
        auth_method: req.provider.auth_method(),
        auth_url,
        redirect_uri,
        expires_at: expires_at.to_rfc3339(),
        completion_mode: "pasteCallbackUrl",
    })
}

pub fn oauth_status(&self, session_id: &str) -> Result<OAuthStatusResponse, AdminServiceError> {
    let session = self.oauth_sessions.get(session_id).ok_or_else(|| {
        AdminServiceError::InvalidCredential("登录会话已过期，请重新开始".to_string())
    })?;
    Ok(session.sanitized_status(Utc::now()))
}

pub fn cancel_oauth_login(&self, session_id: &str) -> Result<(), AdminServiceError> {
    self.oauth_sessions.remove(session_id).ok_or_else(|| {
        AdminServiceError::InvalidCredential("登录会话已过期，请重新开始".to_string())
    })?;
    Ok(())
}
```

- [ ] **Step 6: Implement complete**

Add:

```rust
fn fail_oauth_session(
    &self,
    mut session: OAuthSession,
    error: AdminServiceError,
) -> AdminServiceError {
    session.state_kind = OAuthSessionState::Failed;
    session.error = Some(error.to_string());
    self.oauth_sessions.update(session);
    error
}

fn expire_oauth_session(&self, mut session: OAuthSession) -> AdminServiceError {
    let error = AdminServiceError::InvalidCredential(
        "登录会话已过期，请重新开始".to_string(),
    );
    session.state_kind = OAuthSessionState::Expired;
    session.error = Some(error.to_string());
    session.expires_at = session_expiry(Utc::now());
    self.oauth_sessions.update(session);
    error
}

pub async fn complete_oauth_login(
    &self,
    req: OAuthCompleteRequest,
) -> Result<OAuthCompleteResponse, AdminServiceError> {
    let mut session = self.oauth_sessions.remove(&req.session_id).ok_or_else(|| {
        AdminServiceError::InvalidCredential("登录会话已过期，请重新开始".to_string())
    })?;
    if session.is_expired(Utc::now()) {
        return Err(self.expire_oauth_session(session));
    }

    let callback_url = match req
        .callback_url
        .as_deref()
        .map(str::trim)
        .filter(|callback_url| !callback_url.is_empty())
    {
        Some(callback_url) => callback_url,
        None => {
            let error = AdminServiceError::InvalidCredential(
                "请粘贴 callback URL".to_string(),
            );
            return Err(self.fail_oauth_session(session, error));
        }
    };
    let parsed = match parse_callback_input(callback_url) {
        Ok(parsed) => parsed,
        Err(e) => {
            let error = AdminServiceError::InvalidCredential(e.to_string());
            return Err(self.fail_oauth_session(session, error));
        }
    };

    if parsed.state != session.state {
        let error = AdminServiceError::InvalidCredential(
            "state 不匹配，请重新开始登录".to_string(),
        );
        return Err(self.fail_oauth_session(session, error));
    }

    let code_verifier = match session.code_verifier.take() {
        Some(code_verifier) => code_verifier,
        None => {
            let error = AdminServiceError::InvalidCredential(
                "登录会话已完成或无效".to_string(),
            );
            return Err(self.fail_oauth_session(session, error));
        }
    };
    let config = self.config.read().clone();
    let proxy = self.token_manager.global_proxy();

    let credential = match session.auth_method {
        AuthMethod::Social => {
            let token = match exchange_social_token(
                &parsed.code,
                &code_verifier,
                &session.redirect_uri,
                &session.machine_id,
                &config,
                proxy.as_ref(),
            )
            .await
            {
                Ok(token) => token,
                Err(e) => {
                    let error = self.classify_add_error(e);
                    return Err(self.fail_oauth_session(session, error));
                }
            };
            map_social_credentials(&session, token)
        }
        AuthMethod::Idc => {
            let client_id = match session.client_id.clone() {
                Some(client_id) => client_id,
                None => {
                    let error = AdminServiceError::InvalidCredential(
                        "IdC session missing clientId".to_string(),
                    );
                    return Err(self.fail_oauth_session(session, error));
                }
            };
            let client_secret = match session.client_secret.clone() {
                Some(client_secret) => client_secret,
                None => {
                    let error = AdminServiceError::InvalidCredential(
                        "IdC session missing clientSecret".to_string(),
                    );
                    return Err(self.fail_oauth_session(session, error));
                }
            };
            let token = match exchange_idc_token(
                &session.region,
                &client_id,
                &client_secret,
                &parsed.code,
                &code_verifier,
                &session.redirect_uri,
                &config,
                proxy.as_ref(),
            )
            .await
            {
                Ok(token) => token,
                Err(e) => {
                    let error = self.classify_add_error(e);
                    return Err(self.fail_oauth_session(session, error));
                }
            };
            match map_idc_credentials(&session, token) {
                Ok(credential) => credential,
                Err(e) => {
                    let error = self.classify_add_error(e);
                    return Err(self.fail_oauth_session(session, error));
                }
            }
        }
    };

    let credential_id = match self
        .token_manager
        .add_credential(credential)
        .await
    {
        Ok(credential_id) => credential_id,
        Err(e) => {
            let error = self.classify_add_error(e);
            return Err(self.fail_oauth_session(session, error));
        }
    };

    if let Err(e) = self.token_manager.get_usage_limits_for(credential_id).await {
        tracing::warn!("OAuth 添加凭据后获取订阅等级失败（不影响凭据添加）: {}", e);
    }

    let snapshot = self.token_manager.snapshot();
    let item = snapshot.entries.iter().find(|entry| entry.id == credential_id);
    let email = item.and_then(|entry| entry.email.clone());
    let subscription_title = item.and_then(|entry| entry.subscription_title.clone());
    let supported_model_ids = item
        .map(|entry| {
            crate::kiro::model::capabilities::model_ids_for_subscription(
                entry.subscription_title.as_deref(),
            )
            .iter()
            .map(|id| (*id).to_string())
            .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    session.state_kind = OAuthSessionState::Completed;
    session.credential_id = Some(credential_id);
    session.code_verifier = None;
    session.client_secret = None;
    self.oauth_sessions.update(session);

    Ok(OAuthCompleteResponse {
        success: true,
        credential_id,
        email,
        subscription_title,
        supported_model_ids,
    })
}
```

- [ ] **Step 7: Run service tests**

Run:

```bash
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test oauth_start
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test oauth_cancel
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test oauth_complete_wrong_state_records_failed_status
```

Expected: PASS for start/status/cancel tests and for the failed-session status retention test. Complete success mapping is covered by pure mapper tests and route-level wiring is covered in the next task.

- [ ] **Step 8: Commit Task 3**

```bash
git add src/kiro/token_manager.rs src/admin/service.rs
git commit -m "feat: add admin oauth session service"
```

---

### Task 4: Admin OAuth HTTP Routes

**Files:**
- Modify: `src/admin/handlers.rs`
- Modify: `src/admin/router.rs`

**Interfaces:**
- Consumes: Task 3 service methods and OAuth DTOs.
- Produces: Authenticated Admin API endpoints:
  - `POST /api/admin/oauth/start`
  - `POST /api/admin/oauth/complete`
  - `GET /api/admin/oauth/status/{session_id}`
  - `POST /api/admin/oauth/cancel/{session_id}`

- [ ] **Step 1: Add handlers**

Modify imports in `src/admin/handlers.rs` to include:

```rust
use crate::admin::oauth::{OAuthCompleteRequest, OAuthStartRequest};
```

Add handlers:

```rust
/// POST /api/admin/oauth/start
pub async fn start_oauth_login(
    State(state): State<AdminState>,
    Json(payload): Json<OAuthStartRequest>,
) -> impl IntoResponse {
    match state.service.start_oauth_login(payload).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}

/// POST /api/admin/oauth/complete
pub async fn complete_oauth_login(
    State(state): State<AdminState>,
    Json(payload): Json<OAuthCompleteRequest>,
) -> impl IntoResponse {
    match state.service.complete_oauth_login(payload).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}

/// GET /api/admin/oauth/status/:session_id
pub async fn get_oauth_status(
    State(state): State<AdminState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.service.oauth_status(&session_id) {
        Ok(response) => Json(response).into_response(),
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}

/// POST /api/admin/oauth/cancel/:session_id
pub async fn cancel_oauth_login(
    State(state): State<AdminState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.service.cancel_oauth_login(&session_id) {
        Ok(_) => Json(SuccessResponse::new("OAuth 登录已取消")).into_response(),
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}
```

- [ ] **Step 2: Wire routes**

Modify handler imports in `src/admin/router.rs`:

```rust
cancel_oauth_login, complete_oauth_login, get_oauth_status, start_oauth_login,
```

Add routes before `.layer(...)`:

```rust
.route("/oauth/start", post(start_oauth_login))
.route("/oauth/complete", post(complete_oauth_login))
.route("/oauth/status/{session_id}", get(get_oauth_status))
.route("/oauth/cancel/{session_id}", post(cancel_oauth_login))
```

- [ ] **Step 3: Run backend tests and formatting**

Run:

```bash
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test admin::oauth
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test oauth_start
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test oauth_cancel
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test oauth_complete_wrong_state_records_failed_status
git diff --check
```

Expected: PASS.

- [ ] **Step 4: Commit Task 4**

```bash
git add src/admin/handlers.rs src/admin/router.rs
git commit -m "feat: expose admin oauth endpoints"
```

---

### Task 5: Frontend OAuth API and Hooks

**Files:**
- Modify: `admin-ui/src/types/api.ts`
- Modify: `admin-ui/src/api/credentials.ts`
- Modify: `admin-ui/src/hooks/use-credentials.ts`

**Interfaces:**
- Consumes: Task 4 HTTP endpoints.
- Produces: `OAuthProvider`, `OAuthStartRequest`, `OAuthStartResponse`, `OAuthCompleteRequest`, `OAuthCompleteResponse`, `OAuthStatusResponse`, `startOAuthLogin`, `completeOAuthLogin`, `getOAuthStatus`, `cancelOAuthLogin`, `useStartOAuthLogin`, `useCompleteOAuthLogin`, `useCancelOAuthLogin`.

- [ ] **Step 1: Add TypeScript API types**

Append to `admin-ui/src/types/api.ts`:

```ts
export type OAuthProvider = 'Google' | 'Github' | 'BuilderId' | 'Enterprise'
export type OAuthAuthMethod = 'social' | 'idc'
export type OAuthSessionState = 'pending' | 'completed' | 'failed' | 'cancelled' | 'expired'

export interface OAuthStartRequest {
  provider: OAuthProvider
  region?: string | null
  startUrl?: string | null
  priority?: number
  endpoint?: string | null
  proxyUrl?: string | null
  proxyUsername?: string | null
  proxyPassword?: string | null
}

export interface OAuthStartResponse {
  sessionId: string
  provider: OAuthProvider
  authMethod: OAuthAuthMethod
  authUrl: string
  redirectUri: string
  expiresAt: string
  completionMode: 'pasteCallbackUrl'
}

export interface OAuthCompleteRequest {
  sessionId: string
  callbackUrl?: string | null
  code?: string | null
  state?: string | null
}

export interface OAuthCompleteResponse {
  success: boolean
  credentialId: number
  email?: string | null
  subscriptionTitle?: string | null
  supportedModelIds: string[]
}

export interface OAuthStatusResponse {
  sessionId: string
  state: OAuthSessionState
  provider: OAuthProvider
  expiresAt: string
  credentialId?: number | null
  error?: string | null
}
```

- [ ] **Step 2: Add API functions**

Modify imports in `admin-ui/src/api/credentials.ts`:

```ts
  OAuthStartRequest,
  OAuthStartResponse,
  OAuthCompleteRequest,
  OAuthCompleteResponse,
  OAuthStatusResponse,
```

Append functions:

```ts
export async function startOAuthLogin(req: OAuthStartRequest): Promise<OAuthStartResponse> {
  const { data } = await api.post<OAuthStartResponse>('/oauth/start', req)
  return data
}

export async function completeOAuthLogin(req: OAuthCompleteRequest): Promise<OAuthCompleteResponse> {
  const { data } = await api.post<OAuthCompleteResponse>('/oauth/complete', req)
  return data
}

export async function getOAuthStatus(sessionId: string): Promise<OAuthStatusResponse> {
  const { data } = await api.get<OAuthStatusResponse>(`/oauth/status/${encodeURIComponent(sessionId)}`)
  return data
}

export async function cancelOAuthLogin(sessionId: string): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(`/oauth/cancel/${encodeURIComponent(sessionId)}`)
  return data
}
```

- [ ] **Step 3: Add hooks**

Modify imports in `admin-ui/src/hooks/use-credentials.ts`:

```ts
  startOAuthLogin,
  completeOAuthLogin,
  getOAuthStatus,
  cancelOAuthLogin,
```

Add types:

```ts
  OAuthStartRequest,
  OAuthCompleteRequest,
```

Append hooks:

```ts
export function useStartOAuthLogin() {
  return useMutation({
    mutationFn: (req: OAuthStartRequest) => startOAuthLogin(req),
  })
}

export function useCompleteOAuthLogin() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: OAuthCompleteRequest) => completeOAuthLogin(req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['credentials'] })
      queryClient.invalidateQueries({ queryKey: ['cached-balances'] })
    },
  })
}

export function useOAuthStatus(sessionId: string | null) {
  return useQuery({
    queryKey: ['oauth-status', sessionId],
    queryFn: () => getOAuthStatus(sessionId!),
    enabled: Boolean(sessionId),
    refetchInterval: (query) => {
      const state = query.state.data?.state
      return state === 'pending' ? 2000 : false
    },
  })
}

export function useCancelOAuthLogin() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (sessionId: string) => cancelOAuthLogin(sessionId),
    onSuccess: (_res, sessionId) => {
      queryClient.invalidateQueries({ queryKey: ['oauth-status', sessionId] })
    },
  })
}
```

- [ ] **Step 4: Run frontend build**

Run:

```bash
pnpm --dir admin-ui build
```

Expected: PASS.

- [ ] **Step 5: Commit Task 5**

```bash
git add admin-ui/src/types/api.ts admin-ui/src/api/credentials.ts admin-ui/src/hooks/use-credentials.ts
git commit -m "feat: add admin oauth frontend api"
```

---

### Task 6: OAuth Login Dialog UI

**Files:**
- Create: `admin-ui/src/components/oauth-login-dialog.tsx`

**Interfaces:**
- Consumes: Task 5 hooks and types.
- Produces: `OAuthLoginDialog` React component with provider selection, Enterprise fields, auth URL launch/copy, pasted callback URL completion, and cancel behavior.

- [ ] **Step 1: Create the dialog component**

Create `admin-ui/src/components/oauth-login-dialog.tsx`:

```tsx
import { useMemo, useState } from 'react'
import { Copy, ExternalLink, Loader2, ShieldCheck } from 'lucide-react'
import { toast } from 'sonner'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useCancelOAuthLogin, useCompleteOAuthLogin, useStartOAuthLogin } from '@/hooks/use-credentials'
import { extractErrorMessage } from '@/lib/utils'
import type { OAuthProvider, OAuthStartResponse } from '@/types/api'

interface OAuthLoginDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

const PROVIDERS: Array<{ id: OAuthProvider; label: string; authMethod: 'social' | 'idc' }> = [
  { id: 'Google', label: 'Google', authMethod: 'social' },
  { id: 'Github', label: 'Github', authMethod: 'social' },
  { id: 'BuilderId', label: 'BuilderId', authMethod: 'idc' },
  { id: 'Enterprise', label: 'Enterprise', authMethod: 'idc' },
]

export function OAuthLoginDialog({ open, onOpenChange }: OAuthLoginDialogProps) {
  const [provider, setProvider] = useState<OAuthProvider>('Google')
  const [region, setRegion] = useState('us-east-1')
  const [startUrl, setStartUrl] = useState('')
  const [priority, setPriority] = useState('0')
  const [callbackUrl, setCallbackUrl] = useState('')
  const [session, setSession] = useState<OAuthStartResponse | null>(null)

  const startOAuth = useStartOAuthLogin()
  const completeOAuth = useCompleteOAuthLogin()
  const cancelOAuth = useCancelOAuthLogin()

  const selectedProvider = useMemo(
    () => PROVIDERS.find((item) => item.id === provider) ?? PROVIDERS[0],
    [provider]
  )
  const isEnterprise = provider === 'Enterprise'
  const isBusy = startOAuth.isPending || completeOAuth.isPending || cancelOAuth.isPending

  const reset = () => {
    setProvider('Google')
    setRegion('us-east-1')
    setStartUrl('')
    setPriority('0')
    setCallbackUrl('')
    setSession(null)
  }

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen && session?.sessionId && !completeOAuth.isSuccess) {
      cancelOAuth.mutate(session.sessionId)
    }
    if (!nextOpen) reset()
    onOpenChange(nextOpen)
  }

  const handleStart = () => {
    if (isEnterprise && !startUrl.trim()) {
      toast.error('Enterprise 需要填写 Start URL')
      return
    }
    if (isEnterprise && !/^https:\/\//i.test(startUrl.trim())) {
      toast.error('Enterprise Start URL 必须以 https:// 开头')
      return
    }

    startOAuth.mutate(
      {
        provider,
        region: region.trim() || 'us-east-1',
        startUrl: isEnterprise ? startUrl.trim() : null,
        priority: parseInt(priority, 10) || 0,
      },
      {
        onSuccess: (data) => {
          setSession(data)
          const popup = window.open(data.authUrl, '_blank', 'noopener,noreferrer')
          if (popup) {
            toast.success('已打开授权页面')
          } else {
            toast.info('浏览器阻止了弹窗，请复制授权链接打开')
          }
        },
        onError: (error) => {
          toast.error(`启动 OAuth 登录失败: ${extractErrorMessage(error)}`)
        },
      }
    )
  }

  const copyAuthUrl = async () => {
    if (!session?.authUrl) return
    await navigator.clipboard.writeText(session.authUrl)
    toast.success('授权链接已复制')
  }

  const handleComplete = () => {
    if (!session) {
      toast.error('请先开始 OAuth 登录')
      return
    }
    if (!callbackUrl.trim()) {
      toast.error('请粘贴 callback URL')
      return
    }

    completeOAuth.mutate(
      {
        sessionId: session.sessionId,
        callbackUrl: callbackUrl.trim(),
      },
      {
        onSuccess: (data) => {
          toast.success(`OAuth 凭据添加成功，ID: ${data.credentialId}`)
          handleOpenChange(false)
        },
        onError: (error) => {
          toast.error(`完成 OAuth 登录失败: ${extractErrorMessage(error)}`)
        },
      }
    )
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>OAuth 登录</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="grid grid-cols-2 gap-2">
            {PROVIDERS.map((item) => (
              <Button
                key={item.id}
                type="button"
                variant={provider === item.id ? 'default' : 'outline'}
                onClick={() => setProvider(item.id)}
                disabled={isBusy || Boolean(session)}
              >
                {item.label}
              </Button>
            ))}
          </div>

          <div className="grid grid-cols-2 gap-2">
            <div className="space-y-2">
              <label className="text-sm font-medium" htmlFor="oauth-region">Region</label>
              <Input
                id="oauth-region"
                value={region}
                onChange={(event) => setRegion(event.target.value)}
                disabled={isBusy || Boolean(session)}
              />
            </div>
            <div className="space-y-2">
              <label className="text-sm font-medium" htmlFor="oauth-priority">优先级</label>
              <Input
                id="oauth-priority"
                type="number"
                min="0"
                value={priority}
                onChange={(event) => setPriority(event.target.value)}
                disabled={isBusy || Boolean(session)}
              />
            </div>
          </div>

          {isEnterprise && (
            <div className="space-y-2">
              <label className="text-sm font-medium" htmlFor="oauth-start-url">Enterprise Start URL</label>
              <Input
                id="oauth-start-url"
                placeholder="https://d-xxxxxxxxxx.awsapps.com/start"
                value={startUrl}
                onChange={(event) => setStartUrl(event.target.value)}
                disabled={isBusy || Boolean(session)}
              />
            </div>
          )}

          {session ? (
            <div className="space-y-3 rounded-md border p-3">
              <div className="flex items-center gap-2 text-sm font-medium">
                <ShieldCheck className="h-4 w-4" />
                {selectedProvider.label} 授权已开始
              </div>
              <div className="flex gap-2">
                <Button type="button" variant="outline" onClick={() => window.open(session.authUrl, '_blank', 'noopener,noreferrer')}>
                  <ExternalLink className="mr-2 h-4 w-4" />
                  打开授权页
                </Button>
                <Button type="button" variant="outline" onClick={copyAuthUrl}>
                  <Copy className="mr-2 h-4 w-4" />
                  复制授权链接
                </Button>
              </div>
              <div className="space-y-2">
                <label className="text-sm font-medium" htmlFor="oauth-callback-url">Callback URL</label>
                <Input
                  id="oauth-callback-url"
                  placeholder="粘贴授权完成后的 callback URL"
                  value={callbackUrl}
                  onChange={(event) => setCallbackUrl(event.target.value)}
                  disabled={completeOAuth.isPending}
                />
              </div>
            </div>
          ) : (
            <div className="rounded-md border p-3 text-sm text-muted-foreground">
              OAuth 会打开授权页面。授权完成后，把浏览器显示或跳转出的 callback URL 粘贴回来提交。
            </div>
          )}
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" onClick={() => handleOpenChange(false)} disabled={isBusy}>
            取消
          </Button>
          {session ? (
            <Button type="button" onClick={handleComplete} disabled={isBusy}>
              {completeOAuth.isPending && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              提交 Callback URL
            </Button>
          ) : (
            <Button type="button" onClick={handleStart} disabled={isBusy}>
              {startOAuth.isPending && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              开始授权
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
```

- [ ] **Step 2: Run frontend build**

Run:

```bash
pnpm --dir admin-ui build
```

Expected: PASS.

- [ ] **Step 3: Commit Task 6**

```bash
git add admin-ui/src/components/oauth-login-dialog.tsx
git commit -m "feat: add oauth login dialog"
```

---

### Task 7: Dashboard Integration

**Files:**
- Modify: `admin-ui/src/components/dashboard.tsx`

**Interfaces:**
- Consumes: Task 6 `OAuthLoginDialog`.
- Produces: toolbar entry that opens OAuth login dialog without affecting manual add/import.

- [ ] **Step 1: Add imports and state**

Modify imports:

```tsx
import { KeyRound, RefreshCw, LogOut, Moon, Sun, Server, Plus, Upload, Trash2, RotateCcw, CheckCircle2, Globe, ArrowUp, ArrowDown, Boxes } from 'lucide-react'
import { OAuthLoginDialog } from '@/components/oauth-login-dialog'
```

Add state near other dialog state:

```tsx
const [oauthDialogOpen, setOauthDialogOpen] = useState(false)
```

- [ ] **Step 2: Add toolbar button**

In the credential management action group, place OAuth before manual add:

```tsx
<Button variant="outline" size="sm" onClick={() => setOauthDialogOpen(true)}>
  <KeyRound className="h-4 w-4 mr-2" />
  OAuth 登录
</Button>
```

- [ ] **Step 3: Render dialog**

Near `AddCredentialDialog` and `ImportTokenJsonDialog`, add:

```tsx
<OAuthLoginDialog
  open={oauthDialogOpen}
  onOpenChange={setOauthDialogOpen}
/>
```

- [ ] **Step 4: Run frontend build**

Run:

```bash
pnpm --dir admin-ui build
```

Expected: PASS.

- [ ] **Step 5: Commit Task 7**

```bash
git add admin-ui/src/components/dashboard.tsx
git commit -m "feat: wire oauth login into admin ui"
```

---

### Task 8: Verification, Review, and Runtime Smoke Test

**Files:**
- Modify only if verification finds a concrete issue in changed files.

**Interfaces:**
- Consumes: all previous tasks.
- Produces: verified OAuth implementation ready for review/merge.

- [ ] **Step 1: Run full backend tests**

Run:

```bash
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.88 cargo test
```

Expected: all tests pass.

- [ ] **Step 2: Run frontend build**

Run:

```bash
pnpm --dir admin-ui build
```

Expected: TypeScript and Vite build pass.

- [ ] **Step 3: Run whitespace check**

Run:

```bash
git diff --check
```

Expected: no output.

- [ ] **Step 4: Request Claude Code review**

Run:

```bash
BASE_SHA=$(git rev-parse 7267925)
HEAD_SHA=$(git rev-parse HEAD)
claude -p "Review the diff from $BASE_SHA to $HEAD_SHA for the Admin OAuth login implementation in kiro-rs. Requirements: OAuth endpoints require Admin API key; Social uses pasted kiro:// callback URL; IdC uses pasted callback URL completion; state is verified; codeVerifier/clientSecret/tokens are never returned to frontend or logged; successful OAuth creates KiroCredentials through existing token manager; manual add/import still work. Find correctness, security, and test gaps. Respond in Chinese with blocking issues first."
```

Expected: Claude reports no blocking issues. Fix any Critical or Important issue before continuing.

- [ ] **Step 5: Manual API smoke for start endpoint**

Start the local stack using the repo's normal dev command or Docker compose. Then run with the Admin API key in a shell variable:

```bash
ADMIN_KEY=$(jq -r '.adminApiKey' config/config.json)
curl -fsS -H "x-api-key: $ADMIN_KEY" \
  -H "content-type: application/json" \
  -d '{"provider":"Google","region":"us-east-1","priority":0}' \
  http://127.0.0.1:8990/api/admin/oauth/start \
  | jq '{provider, authMethod, completionMode, redirectUri, authUrlContainsKiro:(.authUrl|contains("redirect_uri=kiro%3A%2F%2Fkiro.kiroAgent%2Fauthenticate-success"))}'
```

Expected:

```json
{
  "provider": "Google",
  "authMethod": "social",
  "completionMode": "pasteCallbackUrl",
  "redirectUri": "kiro://kiro.kiroAgent/authenticate-success",
  "authUrlContainsKiro": true
}
```

- [ ] **Step 6: Manual API smoke for invalid callback**

Use the `sessionId` from Step 5:

```bash
SESSION_ID=$(curl -fsS -H "x-api-key: $ADMIN_KEY" \
  -H "content-type: application/json" \
  -d '{"provider":"Google","region":"us-east-1","priority":0}' \
  http://127.0.0.1:8990/api/admin/oauth/start \
  | jq -r '.sessionId')
curl -sS -H "x-api-key: $ADMIN_KEY" \
  -H "content-type: application/json" \
  -d "{\"sessionId\":\"$SESSION_ID\",\"callbackUrl\":\"kiro://kiro.kiroAgent/authenticate-success?code=abc&state=wrong\"}" \
  http://127.0.0.1:8990/api/admin/oauth/complete \
  | jq '.error.message'
```

Expected string contains:

```text
state 不匹配
```

- [ ] **Step 7: Commit verification fixes**

If verification required fixes:

Stage the concrete files changed by the verification fix, then commit them:

```bash
git status --short
git add src/admin/oauth.rs src/admin/service.rs src/admin/handlers.rs src/admin/router.rs admin-ui/src/types/api.ts admin-ui/src/api/credentials.ts admin-ui/src/hooks/use-credentials.ts admin-ui/src/components/oauth-login-dialog.tsx admin-ui/src/components/dashboard.tsx
git commit -m "fix: address oauth login verification"
```

If `git status --short` shows that only a subset of those files changed, stage only that subset. If no fixes were required, do not create an empty commit.

---

## Self-Review Checklist

- Spec coverage: Tasks 1-4 cover backend OAuth sessions, PKCE, callback parsing, Social/IdC exchanges, Admin API auth path, session TTL, and credential mapping. Tasks 5-7 cover frontend API, dialog, and dashboard integration. Task 8 covers tests, build, Claude review, and runtime smoke.
- Red-flag scan: This plan contains no unfinished markers or unspecified implementation slots.
- Type consistency: `OAuthProvider`, `AuthMethod`, `OAuthStartRequest`, `OAuthStartResponse`, `OAuthCompleteRequest`, `OAuthCompleteResponse`, `OAuthStatusResponse`, and `OAuthSessionStore` are introduced in Task 1 and reused consistently in later tasks.
- Scope check: IdC same-host automatic callback remains outside the first implementation, matching the approved spec.
