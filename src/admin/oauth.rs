#![allow(dead_code)]

use std::collections::HashMap;

use anyhow::bail;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SOCIAL_REDIRECT_URI: &str = "kiro://kiro.kiroAgent/authenticate-success";
pub const BUILDER_ID_START_URL: &str = "https://view.awsapps.com/start";
// KAM v1.8.9+ intentionally registers the AWS SSO OIDC client with a
// no-port loopback redirect URI, then uses the real local callback port for
// authorize/token. The no-port registration is required to get a client that
// supports refresh-token grants.
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

#[derive(Clone, Deserialize)]
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

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthCompleteRequest {
    pub session_id: String,
    pub callback_url: Option<String>,
    pub code: Option<String>,
    pub state: Option<String>,
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

#[derive(Clone, PartialEq, Eq)]
pub struct ParsedCallback {
    pub code: String,
    pub state: String,
}

#[derive(Clone)]
pub struct PkcePair {
    pub verifier: String,
    pub challenge: String,
}

#[derive(Clone)]
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

    pub fn clear_sensitive_fields(&mut self) {
        self.state.clear();
        self.code_verifier = None;
        self.client_secret = None;
        self.proxy_url = None;
        self.proxy_username = None;
        self.proxy_password = None;
    }

    pub fn mark_completed(&mut self, credential_id: u64) {
        self.state_kind = OAuthSessionState::Completed;
        self.credential_id = Some(credential_id);
        self.error = None;
        self.clear_sensitive_fields();
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

#[derive(Default)]
pub struct OAuthSessionStore {
    sessions: Mutex<HashMap<String, OAuthSession>>,
}

impl OAuthSessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, session: OAuthSession) {
        self.prune_expired();
        if session.is_expired(Utc::now()) {
            return;
        }
        self.sessions
            .lock()
            .insert(session.session_id.clone(), session);
    }

    pub fn get(&self, session_id: &str) -> Option<OAuthSession> {
        self.prune_expired();
        self.sessions.lock().get(session_id).cloned()
    }

    pub fn update(&self, mut session: OAuthSession) {
        self.prune_expired();
        if session.is_expired(Utc::now()) {
            self.sessions.lock().remove(&session.session_id);
            return;
        }
        if matches!(
            session.state_kind,
            OAuthSessionState::Completed
                | OAuthSessionState::Failed
                | OAuthSessionState::Cancelled
                | OAuthSessionState::Expired
        ) {
            session.clear_sensitive_fields();
        }
        self.sessions
            .lock()
            .insert(session.session_id.clone(), session);
    }

    pub fn remove(&self, session_id: &str) -> Option<OAuthSession> {
        self.sessions.lock().remove(session_id)
    }

    fn prune_expired(&self) {
        let now = Utc::now();
        self.sessions
            .lock()
            .retain(|_, session| !session.is_expired(now));
    }
}

fn random_bytes<const N: usize>() -> anyhow::Result<[u8; N]> {
    let mut bytes = [0_u8; N];
    getrandom::getrandom(&mut bytes)
        .map_err(|err| anyhow::anyhow!("OS randomness unavailable: {err}"))?;
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

    let params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();

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

pub fn build_social_auth_url(provider: OAuthProvider, code_challenge: &str, state: &str) -> String {
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
        assert!(
            pair.verifier
                .bytes()
                .all(|b| { b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_') })
        );
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
        let err = match parse_callback_input(
            "kiro://kiro.kiroAgent/authenticate-success?state=state-1",
        ) {
            Ok(_) => panic!("callback without code should fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("回调 URL 缺少 code"));
    }

    #[test]
    fn social_auth_url_uses_kiro_deep_link_redirect() {
        let url = build_social_auth_url(OAuthProvider::Google, "challenge", "state");
        assert!(url.contains("idp=Google"));
        assert!(url.contains("redirect_uri=kiro%3A%2F%2Fkiro.kiroAgent%2Fauthenticate-success"));
        assert!(url.contains("code_challenge=challenge"));
        assert!(url.contains("state=state"));
    }

    #[test]
    fn completed_session_clears_sensitive_fields() {
        let mut session = test_session();
        session.client_secret = Some("client-secret".to_string());
        session.proxy_url = Some("https://proxy-user:proxy-pass@proxy.example".to_string());
        session.proxy_username = Some("proxy-user".to_string());
        session.proxy_password = Some("proxy-pass".to_string());

        session.mark_completed(42);

        assert_eq!(session.state_kind, OAuthSessionState::Completed);
        assert_eq!(session.credential_id, Some(42));
        assert!(session.state.is_empty());
        assert!(session.code_verifier.is_none());
        assert!(session.client_secret.is_none());
        assert!(session.proxy_url.is_none());
        assert!(session.proxy_username.is_none());
        assert!(session.proxy_password.is_none());
    }

    #[test]
    fn session_store_prunes_expired_completed_sessions() {
        let store = OAuthSessionStore::new();
        let mut session = test_session();
        session.expires_at = Utc::now() - Duration::seconds(1);
        session.mark_completed(7);
        let id = session.session_id.clone();

        store.update(session);

        assert!(store.get(&id).is_none());
    }

    #[test]
    fn session_store_does_not_insert_expired_sessions() {
        let store = OAuthSessionStore::new();
        let mut session = test_session();
        session.expires_at = Utc::now() - Duration::seconds(1);
        let id = session.session_id.clone();

        store.insert(session);

        assert!(store.sessions.lock().get(&id).is_none());
    }

    #[test]
    fn idc_authorize_url_matches_kiro_scopes_parameter() {
        let url = build_idc_authorize_url(
            "us-east-1",
            "client",
            "http://127.0.0.1:49152/oauth/callback",
            "challenge",
            "state",
        );

        assert!(url.contains("/authorize?response_type=code"));
        assert!(url.contains("client_id=client"));
        assert!(url.contains("scopes=codewhisperer%3Acompletions%2Ccodewhisperer%3Aanalysis"));
        assert!(!url.contains("&scope="));
        assert!(url.contains("code_challenge=challenge"));
    }

    #[test]
    fn idc_registration_and_authorize_redirects_match_kiro_flow() {
        assert_eq!(
            IDC_REGISTER_REDIRECT_URI,
            "http://127.0.0.1/oauth/callback"
        );
        assert_eq!(
            idc_callback_redirect_uri(),
            "http://127.0.0.1:49152/oauth/callback"
        );
    }

    fn test_session() -> OAuthSession {
        OAuthSession {
            session_id: "session-1".to_string(),
            provider: OAuthProvider::Google,
            auth_method: AuthMethod::Social,
            state: "state-1".to_string(),
            code_verifier: Some("verifier-1".to_string()),
            redirect_uri: SOCIAL_REDIRECT_URI.to_string(),
            region: "us-east-1".to_string(),
            start_url: None,
            client_id: None,
            client_secret: None,
            machine_id: "machine-1".to_string(),
            priority: 0,
            endpoint: None,
            proxy_url: None,
            proxy_username: None,
            proxy_password: None,
            created_at: Utc::now(),
            expires_at: session_expiry(Utc::now()),
            state_kind: OAuthSessionState::Pending,
            credential_id: None,
            error: None,
        }
    }
}
