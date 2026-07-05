#![allow(dead_code)]

use std::collections::HashMap;

use anyhow::{Context, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::http_client::{ProxyConfig, build_client};
use crate::kiro::model::credentials::KiroCredentials;
use crate::model::config::Config;

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

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStartResponse {
    pub session_id: String,
    pub provider: OAuthProvider,
    pub auth_method: AuthMethod,
    pub auth_url: String,
    /// Static provider redirect URI, without code/state or pasted callback data.
    pub redirect_uri: String,
    pub expires_at: String,
    pub completion_mode: &'static str,
}

#[derive(Clone, Deserialize)]
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

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state_kind,
            OAuthSessionState::Completed
                | OAuthSessionState::Failed
                | OAuthSessionState::Cancelled
                | OAuthSessionState::Expired
        )
    }

    pub fn clear_sensitive_fields(&mut self) {
        self.state.clear();
        self.code_verifier = None;
        self.redirect_uri.clear();
        self.region.clear();
        self.start_url = None;
        self.client_id = None;
        self.client_secret = None;
        self.machine_id.clear();
        self.priority = 0;
        self.endpoint = None;
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

    pub fn insert(&self, mut session: OAuthSession) {
        self.prune_expired();
        if session.is_expired(Utc::now()) {
            return;
        }
        if session.is_terminal() {
            session.clear_sensitive_fields();
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
        if session.is_terminal() {
            session.clear_sensitive_fields();
        }
        self.sessions
            .lock()
            .insert(session.session_id.clone(), session);
    }

    pub fn remove(&self, session_id: &str) -> Option<OAuthSession> {
        self.sessions.lock().remove(session_id)
    }

    #[cfg(test)]
    pub(crate) fn insert_for_test(&self, session: OAuthSession) {
        self.sessions
            .lock()
            .insert(session.session_id.clone(), session);
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

    let url =
        url::Url::parse(trimmed).map_err(|_| anyhow::anyhow!("请粘贴完整 callback URL"))?;
    let query = url.query().unwrap_or("").to_string();

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

pub fn validate_callback_redirect(input: &str, expected_redirect_uri: &str) -> anyhow::Result<()> {
    let actual =
        url::Url::parse(input.trim()).map_err(|_| anyhow::anyhow!("请粘贴完整 callback URL"))?;
    let expected = url::Url::parse(expected_redirect_uri)
        .map_err(|_| anyhow::anyhow!("OAuth session redirect URI 无效"))?;

    let matches_redirect = actual.scheme() == expected.scheme()
        && actual.host_str() == expected.host_str()
        && actual.port_or_known_default() == expected.port_or_known_default()
        && actual.path() == expected.path()
        && actual.username().is_empty()
        && actual.password().is_none()
        && actual.fragment().is_none();

    if !matches_redirect {
        bail!("callback URL 与当前登录会话不匹配");
    }

    Ok(())
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

#[derive(Clone, Deserialize)]
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

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdcClientRegistration {
    #[serde(rename = "clientId")]
    pub client_id: String,
    #[serde(rename = "clientSecret")]
    pub client_secret: String,
}

#[derive(Clone, Deserialize)]
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
        runtime_only: false,
        id: None,
        access_token: Some(token.access_token),
        refresh_token: Some(token.refresh_token),
        kiro_api_key: None,
        profile_arn: token.profile_arn,
        expires_at: expires_at_from_now(token.expires_in),
        auth_method: Some(session.auth_method.as_credential_value().to_string()),
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
        runtime_only: false,
        id: None,
        access_token: Some(token.access_token),
        refresh_token: Some(token.refresh_token),
        kiro_api_key: None,
        profile_arn: None,
        expires_at: expires_at_from_now(token.expires_in),
        auth_method: Some(session.auth_method.as_credential_value().to_string()),
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
    })
}

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
        bail!("Token 交换失败，请重新授权 (HTTP {})", status);
    }
    let text = resp
        .text()
        .await
        .context("Social token exchange response read failed")?;

    serde_json::from_str(&text).context("Social token exchange response parse failed")
}

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
            status
        );
    }
    let text = resp
        .text()
        .await
        .context("AWS SSO client registration response read failed")?;

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
        bail!("Token 交换失败，请重新授权 (HTTP {})", status);
    }
    let text = resp
        .text()
        .await
        .context("AWS SSO token exchange response read failed")?;

    serde_json::from_str(&text).context("AWS SSO token exchange parse failed")
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
    fn callback_parser_rejects_raw_query_string() {
        let err = match parse_callback_input("code=abc&state=state-1") {
            Ok(_) => panic!("raw query string should fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("完整 callback URL"));
    }

    #[test]
    fn callback_redirect_validator_rejects_wrong_social_url() {
        let err = match validate_callback_redirect(
            "http://127.0.0.1:8990/oauth/callback?code=abc&state=state-1",
            SOCIAL_REDIRECT_URI,
        ) {
            Ok(_) => panic!("wrong callback redirect should fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("不匹配"));
    }

    #[test]
    fn callback_redirect_validator_rejects_fragment() {
        let err = match validate_callback_redirect(
            "kiro://kiro.kiroAgent/authenticate-success?code=abc&state=state-1#extra",
            SOCIAL_REDIRECT_URI,
        ) {
            Ok(_) => panic!("fragment-bearing callback redirect should fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("不匹配"));
    }

    #[test]
    fn completed_session_clears_stale_error() {
        let mut session = test_session();
        session.error = Some("state 不匹配，请重新开始登录".to_string());

        session.mark_completed(42);

        assert_eq!(session.state_kind, OAuthSessionState::Completed);
        assert_eq!(session.credential_id, Some(42));
        assert!(session.error.is_none());
        assert!(session.state.is_empty());
        assert!(session.code_verifier.is_none());
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
        assert!(session.redirect_uri.is_empty());
        assert!(session.region.is_empty());
        assert!(session.start_url.is_none());
        assert!(session.client_id.is_none());
        assert!(session.client_secret.is_none());
        assert!(session.machine_id.is_empty());
        assert_eq!(session.priority, 0);
        assert!(session.endpoint.is_none());
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
    fn session_store_scrubs_terminal_sessions_on_insert() {
        let store = OAuthSessionStore::new();
        let mut session = test_session();
        session.state_kind = OAuthSessionState::Completed;
        session.credential_id = Some(12);
        let id = session.session_id.clone();

        store.insert(session);

        let stored = store.sessions.lock().get(&id).cloned().unwrap();
        assert_eq!(stored.state_kind, OAuthSessionState::Completed);
        assert_eq!(stored.credential_id, Some(12));
        assert!(stored.state.is_empty());
        assert!(stored.code_verifier.is_none());
        assert!(stored.redirect_uri.is_empty());
        assert!(stored.machine_id.is_empty());
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
