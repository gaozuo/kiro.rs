//! Admin API 业务逻辑服务

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::anthropic::PromptCacheRuntime;
use crate::common::utf8::floor_char_boundary;
use crate::http_client::ProxyConfig;
use crate::kiro::model::credentials::KiroCredentials;
use crate::kiro::provider::KiroProvider;
use crate::kiro::token_manager::{CachedBalanceInfo, MultiTokenManager};
use crate::model::config::{CompressionConfig, Config};
use parking_lot::RwLock;

use super::error::AdminServiceError;
use super::oauth::{
    AuthMethod, BUILDER_ID_START_URL, OAuthCompleteRequest, OAuthCompleteResponse, OAuthProvider,
    OAuthSession, OAuthSessionState, OAuthSessionStore, OAuthStartRequest, OAuthStartResponse,
    OAuthStatusResponse, SOCIAL_REDIRECT_URI, build_idc_authorize_url, build_social_auth_url,
    exchange_idc_token, exchange_social_token, generate_machine_id, generate_pkce_pair,
    generate_session_id, generate_state, idc_callback_redirect_uri, map_idc_credentials,
    map_social_credentials, parse_callback_input, register_idc_client, session_expiry,
};
use super::types::{
    AddCredentialRequest, AddCredentialResponse, BalanceResponse, CachedBalanceItem,
    CachedBalancesResponse, CredentialStatusItem, CredentialsStatusResponse, ImportAction,
    ImportItemResult, ImportSummary, ImportTokenJsonRequest, ImportTokenJsonResponse,
    ProxyConfigResponse, TokenJsonItem, UpdateProxyConfigRequest,
};

/// 余额缓存过期时间（秒），5 分钟
const BALANCE_CACHE_TTL_SECS: i64 = 300;

/// 缓存的余额条目（含时间戳）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedBalance {
    /// 缓存时间（Unix 秒）
    cached_at: f64,
    /// 缓存的余额数据
    data: BalanceResponse,
}

/// Admin 服务
///
/// 封装所有 Admin API 的业务逻辑
pub struct AdminService {
    token_manager: Arc<MultiTokenManager>,
    kiro_provider: Option<Arc<KiroProvider>>,
    config: Arc<RwLock<Config>>,
    compression_config: Arc<RwLock<CompressionConfig>>,
    prompt_cache_runtime: Arc<RwLock<PromptCacheRuntime>>,
    balance_cache: Mutex<HashMap<u64, CachedBalance>>,
    cache_path: Option<PathBuf>,
    known_endpoints: HashSet<String>,
    oauth_sessions: OAuthSessionStore,
}

impl AdminService {
    pub fn new(
        token_manager: Arc<MultiTokenManager>,
        kiro_provider: Option<Arc<KiroProvider>>,
        config: Arc<RwLock<Config>>,
        compression_config: Arc<RwLock<CompressionConfig>>,
        prompt_cache_runtime: Arc<RwLock<PromptCacheRuntime>>,
        known_endpoints: impl IntoIterator<Item = String>,
    ) -> Self {
        let cache_path = token_manager
            .cache_dir()
            .map(|d| d.join("kiro_balance_cache.json"));

        let balance_cache = Self::load_balance_cache_from(&cache_path);

        for (id, cached) in &balance_cache {
            token_manager.restore_balance_cache(*id, cached.data.remaining, cached.cached_at);
        }

        Self {
            token_manager,
            kiro_provider,
            config,
            compression_config,
            prompt_cache_runtime,
            balance_cache: Mutex::new(balance_cache),
            cache_path,
            known_endpoints: known_endpoints.into_iter().collect(),
            oauth_sessions: OAuthSessionStore::new(),
        }
    }

    /// 获取所有凭据状态
    pub fn get_all_credentials(&self) -> CredentialsStatusResponse {
        let snapshot = self.token_manager.snapshot();

        let default_endpoint = self.config.read().default_endpoint.clone();
        let available_model_ids = crate::kiro::model::capabilities::union_model_ids_for_subscriptions(
            snapshot
                .entries
                .iter()
                .filter(|entry| !entry.disabled)
                .map(|entry| entry.subscription_title.clone()),
        )
            .into_iter()
            .map(str::to_string)
            .collect();
        let mut credentials: Vec<CredentialStatusItem> = snapshot
            .entries
            .into_iter()
            .map(|entry| {
                let endpoint = entry.endpoint;
                let effective_endpoint = endpoint.clone().unwrap_or(default_endpoint.clone());
                let supported_model_ids: Vec<String> =
                    crate::kiro::model::capabilities::model_ids_for_subscription(
                        entry.subscription_title.as_deref(),
                    )
                    .iter()
                    .map(|id| (*id).to_string())
                    .collect();
                let supported_model_count = supported_model_ids.len();
                CredentialStatusItem {
                    id: entry.id,
                    priority: entry.priority,
                    disabled: entry.disabled,
                    failure_count: entry.failure_count,
                    refresh_failure_count: entry.refresh_failure_count,
                    disabled_reason: entry.disable_reason.map(|reason| format!("{:?}", reason)),
                    expires_at: entry.expires_at,
                    auth_method: entry.auth_method,
                    has_profile_arn: entry.has_profile_arn,
                    refresh_token_hash: entry.refresh_token_hash,
                    email: entry.email,
                    subscription_title: entry.subscription_title,
                    supported_model_ids,
                    supported_model_count,
                    success_count: entry.success_count,
                    last_used_at: entry.last_used_at.clone(),
                    region: entry.region,
                    api_region: entry.api_region,
                    endpoint,
                    effective_endpoint,
                    idp: entry.idp,
                    proxy_url: entry.proxy_url,
                    proxy_username: entry.proxy_username,
                    has_proxy_password: entry.has_proxy_password,
                    overage_enabled: entry.overage_enabled,
                    overage_enabling: entry.overage_enabling,
                    overage_last_error: entry.overage_last_error,
                }
            })
            .collect();

        // 按优先级排序（数字越小优先级越高）
        credentials.sort_by_key(|c| c.priority);

        CredentialsStatusResponse {
            total: snapshot.total,
            available: snapshot.available,
            available_model_ids,
            credentials,
        }
    }

    /// 设置凭据禁用状态
    pub fn set_disabled(&self, id: u64, disabled: bool) -> Result<(), AdminServiceError> {
        self.token_manager
            .set_disabled(id, disabled)
            .map_err(|e| self.classify_error(e, id))
    }

    /// 设置凭据优先级
    pub fn set_priority(&self, id: u64, priority: u32) -> Result<(), AdminServiceError> {
        self.token_manager
            .set_priority(id, priority)
            .map_err(|e| self.classify_error(e, id))
    }

    /// 设置凭据 Region
    pub fn set_region(
        &self,
        id: u64,
        region: Option<String>,
        api_region: Option<String>,
    ) -> Result<(), AdminServiceError> {
        // trim 后空字符串转 None
        let region = region
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let api_region = api_region
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        self.token_manager
            .set_region(id, region, api_region)
            .map_err(|e| self.classify_error(e, id))
    }

    /// 设置凭据 endpoint
    pub fn set_endpoint(&self, id: u64, endpoint: Option<String>) -> Result<(), AdminServiceError> {
        let endpoint = self.validate_oauth_endpoint(endpoint)?;

        self.token_manager
            .set_endpoint(id, endpoint)
            .map_err(|e| self.classify_error(e, id))
    }

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

    fn validate_oauth_endpoint(
        &self,
        endpoint: Option<String>,
    ) -> Result<Option<String>, AdminServiceError> {
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

    /// 设置凭据级 Web Portal Idp
    pub fn set_idp(&self, id: u64, idp: Option<String>) -> Result<(), AdminServiceError> {
        self.token_manager
            .set_idp_for(id, idp)
            .map_err(|e| self.classify_error(e, id))
    }

    /// 设置凭据级代理
    pub fn set_credential_proxy(
        &self,
        id: u64,
        proxy_url: Option<String>,
        proxy_username: Option<String>,
        proxy_password: Option<String>,
    ) -> Result<(), AdminServiceError> {
        self.token_manager
            .set_proxy_for(id, proxy_url, proxy_username, proxy_password)
            .map_err(|e| self.classify_error(e, id))
    }

    /// 读取凭据级 overage 状态
    pub fn overage_status(
        &self,
        id: u64,
    ) -> Result<crate::kiro::token_manager::OverageStatusSnapshot, AdminServiceError> {
        self.token_manager
            .overage_status_for(id)
            .map_err(|e| self.classify_error(e, id))
    }

    /// 尝试占用 overage 任务执行权
    pub fn try_begin_overage_task(&self, id: u64) -> Result<bool, AdminServiceError> {
        self.token_manager
            .try_begin_overage_task(id)
            .map_err(|e| self.classify_error(e, id))
    }

    /// 暴露 MultiTokenManager 句柄供 overage SSE 流持有
    ///
    /// SSE handler 需要把 Arc 跨 tokio::spawn 传给后台轮询任务，
    /// 因此这里返回 Arc 而不是 &MultiTokenManager。
    pub fn token_manager_arc(&self) -> Arc<MultiTokenManager> {
        Arc::clone(&self.token_manager)
    }

    /// 重置失败计数并重新启用
    pub fn reset_and_enable(&self, id: u64) -> Result<(), AdminServiceError> {
        self.token_manager
            .reset_and_enable(id)
            .map_err(|e| self.classify_error(e, id))
    }

    /// 强制刷新指定凭据 Token
    pub async fn force_refresh_token(&self, id: u64) -> Result<(), AdminServiceError> {
        self.token_manager
            .force_refresh_token_for(id)
            .await
            .map_err(|e| self.classify_error(e, id))
    }

    /// 获取凭据余额（带缓存）
    pub async fn get_balance(&self, id: u64) -> Result<BalanceResponse, AdminServiceError> {
        // 先查缓存。
        // 但如果缓存里的 overage 状态为 false，而凭据列表里已经从其它路径
        // （例如 overage SSE 轮询 / GetUserUsageAndLimits 同步）知道它是 true，
        // 这个缓存就会把“已开启”覆盖成“未开启”。此时必须跳过缓存重新查上游。
        {
            let cache = self.balance_cache.lock();
            if let Some(cached) = cache.get(&id) {
                let now = Utc::now().timestamp() as f64;
                if (now - cached.cached_at) < BALANCE_CACHE_TTL_SECS as f64 {
                    let known_overage_enabled = self
                        .token_manager
                        .overage_status_for(id)
                        .ok()
                        .and_then(|s| s.enabled)
                        .unwrap_or(false);
                    if known_overage_enabled && !cached.data.overage_enabled {
                        tracing::debug!("凭据 #{} 余额缓存 overage 状态过旧，重新查询上游", id);
                    } else {
                        tracing::debug!("凭据 #{} 余额命中缓存", id);
                        return Ok(cached.data.clone());
                    }
                }
            }
        }

        // 缓存未命中或已过期，从上游获取
        let balance = self.fetch_balance(id).await?;

        // 更新缓存
        {
            let mut cache = self.balance_cache.lock();
            cache.insert(
                id,
                CachedBalance {
                    cached_at: Utc::now().timestamp() as f64,
                    data: balance.clone(),
                },
            );
        }
        self.save_balance_cache();

        Ok(balance)
    }

    /// 从上游获取余额（无缓存）
    async fn fetch_balance(&self, id: u64) -> Result<BalanceResponse, AdminServiceError> {
        let usage = self
            .token_manager
            .get_usage_limits_for(id)
            .await
            .map_err(|e| self.classify_balance_error(e, id))?;

        let current_usage = usage.current_usage();
        let overage_enabled = usage
            .overage_enabled_reported()
            .unwrap_or_else(|| self.token_manager.is_overage_known_enabled(id));
        let usage_limit = usage.effective_usage_limit_for_overage(overage_enabled);
        let overage_cap = usage.overage_cap_for_enabled(overage_enabled);
        let remaining = usage.remaining_balance_for_overage(overage_enabled);
        let usage_percentage = if usage_limit > 0.0 {
            (current_usage / usage_limit * 100.0).min(100.0)
        } else {
            0.0
        };

        // 更新缓存，使列表页面能显示最新余额
        self.token_manager.update_balance_cache_details(
            id,
            remaining,
            usage_limit,
            Some(overage_enabled),
            Some(overage_cap),
        );

        Ok(BalanceResponse {
            id,
            subscription_title: usage.subscription_title().map(|s| s.to_string()),
            current_usage,
            usage_limit,
            remaining,
            usage_percentage,
            next_reset_at: usage.next_date_reset,
            overage_enabled,
            overage_cap,
        })
    }

    /// 获取所有凭据的缓存余额
    pub fn get_cached_balances(&self) -> CachedBalancesResponse {
        // 从 token_manager 获取运行时缓存（含 TTL 信息）
        let runtime_balances: HashMap<u64, CachedBalanceInfo> = self
            .token_manager
            .get_all_cached_balances()
            .into_iter()
            .map(|info| (info.id, info))
            .collect();

        // 从 AdminService 磁盘缓存获取完整余额信息
        let disk_cache = self.balance_cache.lock();

        let balances = runtime_balances
            .into_iter()
            .map(|(id, info)| {
                // 优先从磁盘缓存获取完整快照（保证字段一致性）
                if let Some(cached) = disk_cache.get(&id) {
                    CachedBalanceItem {
                        id,
                        remaining: cached.data.remaining,
                        usage_limit: cached.data.usage_limit,
                        usage_percentage: cached.data.usage_percentage,
                        subscription_title: cached.data.subscription_title.clone(),
                        cached_at: info.cached_at,
                        ttl_secs: info.ttl_secs,
                        overage_enabled: cached.data.overage_enabled,
                        overage_cap: cached.data.overage_cap,
                    }
                } else {
                    CachedBalanceItem {
                        id,
                        remaining: info.remaining,
                        usage_limit: info.usage_limit,
                        usage_percentage: if info.usage_limit > 0.0 {
                            ((info.usage_limit - info.remaining).max(0.0) / info.usage_limit
                                * 100.0)
                                .min(100.0)
                        } else {
                            0.0
                        },
                        subscription_title: None,
                        cached_at: info.cached_at,
                        ttl_secs: info.ttl_secs,
                        overage_enabled: info.overage_enabled,
                        overage_cap: info.overage_cap,
                    }
                }
            })
            .collect();

        CachedBalancesResponse { balances }
    }

    /// 添加新凭据
    pub async fn add_credential(
        &self,
        req: AddCredentialRequest,
    ) -> Result<AddCredentialResponse, AdminServiceError> {
        // 构建凭据对象
        let email = req.email.clone();
        let effective_auth_method = if req
            .kiro_api_key
            .as_deref()
            .is_some_and(|key| !key.trim().is_empty())
        {
            "api_key".to_string()
        } else {
            req.auth_method.clone()
        };
        let endpoint = req
            .endpoint
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
        let new_cred = KiroCredentials {
            id: None,
            access_token: None,
            refresh_token: req.refresh_token,
            kiro_api_key: req.kiro_api_key,
            profile_arn: None,
            expires_at: None,
            auth_method: Some(effective_auth_method),
            client_id: req.client_id,
            client_secret: req.client_secret,
            priority: req.priority,
            region: req.region,
            api_region: req.api_region,
            machine_id: req.machine_id,
            endpoint,
            idp: None,
            overage_enabled: None,
            email: req.email,
            subscription_title: None,
            proxy_url: req.proxy_url,
            proxy_username: req.proxy_username,
            proxy_password: req.proxy_password,
            disabled: false, // 新添加的凭据默认启用
            runtime_only: false,
        };

        // 调用 token_manager 添加凭据
        let credential_id = self
            .token_manager
            .add_credential(new_cred)
            .await
            .map_err(|e| self.classify_add_error(e))?;

        if let Err(e) = self.token_manager.get_usage_limits_for(credential_id).await {
            tracing::warn!("添加凭据后获取订阅等级失败（不影响凭据添加）: {}", e);
        }

        Ok(AddCredentialResponse {
            success: true,
            message: format!("凭据添加成功，ID: {}", credential_id),
            credential_id,
            email,
        })
    }

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

    pub async fn complete_oauth_login(
        &self,
        req: OAuthCompleteRequest,
    ) -> Result<OAuthCompleteResponse, AdminServiceError> {
        let mut session = self.oauth_sessions.remove(&req.session_id).ok_or_else(|| {
            AdminServiceError::InvalidCredential("登录会话已过期，请重新开始".to_string())
        })?;
        if session.is_expired(Utc::now()) {
            return Err(AdminServiceError::InvalidCredential(
                "登录会话已过期，请重新开始".to_string(),
            ));
        }

        let parsed = match req.callback_url.as_deref() {
            Some(callback_url) => parse_callback_input(callback_url)
                .map_err(|e| AdminServiceError::InvalidCredential(e.to_string()))?,
            None => {
                let code = req
                    .code
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        AdminServiceError::InvalidCredential("回调 URL 缺少 code".to_string())
                    })?;
                let state = req
                    .state
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        AdminServiceError::InvalidCredential("回调 URL 缺少 state".to_string())
                    })?;
                crate::admin::oauth::ParsedCallback {
                    code: code.to_string(),
                    state: state.to_string(),
                }
            }
        };

        if parsed.state != session.state {
            return Err(AdminServiceError::InvalidCredential(
                "state 不匹配，请重新开始登录".to_string(),
            ));
        }

        let code_verifier = session.code_verifier.take().ok_or_else(|| {
            AdminServiceError::InvalidCredential("登录会话已完成或无效".to_string())
        })?;
        let config = self.config.read().clone();
        let proxy = self.token_manager.global_proxy();

        let credential = match session.auth_method {
            AuthMethod::Social => {
                let token = exchange_social_token(
                    &parsed.code,
                    &code_verifier,
                    &session.redirect_uri,
                    &session.machine_id,
                    &config,
                    proxy.as_ref(),
                )
                .await
                .map_err(|e| self.classify_add_error(e))?;
                map_social_credentials(&session, token)
            }
            AuthMethod::Idc => {
                let client_id = session.client_id.clone().ok_or_else(|| {
                    AdminServiceError::InvalidCredential("IdC session missing clientId".to_string())
                })?;
                let client_secret = session.client_secret.clone().ok_or_else(|| {
                    AdminServiceError::InvalidCredential(
                        "IdC session missing clientSecret".to_string(),
                    )
                })?;
                let token = exchange_idc_token(
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
                .map_err(|e| self.classify_add_error(e))?;
                map_idc_credentials(&session, token).map_err(|e| self.classify_add_error(e))?
            }
        };

        let credential_id = self
            .token_manager
            .add_credential(credential)
            .await
            .map_err(|e| self.classify_add_error(e))?;

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

    /// 删除凭据
    pub fn delete_credential(&self, id: u64) -> Result<(), AdminServiceError> {
        self.token_manager
            .delete_credential(id)
            .map_err(|e| self.classify_delete_error(e, id))?;

        // 清理已删除凭据的余额缓存
        {
            let mut cache = self.balance_cache.lock();
            cache.remove(&id);
        }
        self.save_balance_cache();

        Ok(())
    }

    // ============ 余额缓存持久化 ============

    fn load_balance_cache_from(cache_path: &Option<PathBuf>) -> HashMap<u64, CachedBalance> {
        let path = match cache_path {
            Some(p) => p,
            None => return HashMap::new(),
        };

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return HashMap::new(),
        };

        // 文件中使用字符串 key 以兼容 JSON 格式
        let map: HashMap<String, CachedBalance> = match serde_json::from_str(&content) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("解析余额缓存失败，将忽略: {}", e);
                return HashMap::new();
            }
        };

        let now = Utc::now().timestamp() as f64;
        map.into_iter()
            .filter_map(|(k, v)| {
                let id = k.parse::<u64>().ok()?;
                // 丢弃超过 TTL 的条目
                if (now - v.cached_at) < BALANCE_CACHE_TTL_SECS as f64 {
                    Some((id, v))
                } else {
                    None
                }
            })
            .collect()
    }

    fn save_balance_cache(&self) {
        let path = match &self.cache_path {
            Some(p) => p,
            None => return,
        };

        // 快速 clone 数据后释放锁，减少锁持有时间
        let map: HashMap<String, CachedBalance> = {
            let cache = self.balance_cache.lock();
            cache
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect()
        };

        // 锁外执行序列化和文件 IO
        match serde_json::to_string_pretty(&map) {
            Ok(json) => {
                // 原子写入：先写临时文件，再重命名
                let tmp_path = path.with_extension("json.tmp");
                match std::fs::write(&tmp_path, json) {
                    Ok(_) => {
                        if let Err(e) = std::fs::rename(&tmp_path, path) {
                            tracing::warn!("原子重命名余额缓存失败: {}", e);
                            let _ = std::fs::remove_file(&tmp_path);
                        }
                    }
                    Err(e) => tracing::warn!("写入临时余额文件失败: {}", e),
                }
            }
            Err(e) => tracing::warn!("序列化余额缓存失败: {}", e),
        }
    }

    // ============ 错误分类 ============

    /// 分类简单操作错误（set_disabled, set_priority, reset_and_enable）
    fn classify_error(&self, e: anyhow::Error, id: u64) -> AdminServiceError {
        let msg = e.to_string();
        if msg.contains("API Key 凭据无需刷新 Token") {
            AdminServiceError::InvalidCredential(msg)
        } else if msg.contains("不存在") {
            AdminServiceError::NotFound { id }
        } else {
            AdminServiceError::InternalError(msg)
        }
    }

    /// 分类余额查询错误（可能涉及上游 API 调用）
    fn classify_balance_error(&self, e: anyhow::Error, id: u64) -> AdminServiceError {
        let msg = e.to_string();

        // 1. 凭据不存在
        if msg.contains("不存在") {
            return AdminServiceError::NotFound { id };
        }

        // 2. 上游服务错误特征：HTTP 响应错误或网络错误
        let is_upstream_error =
            // HTTP 响应错误（来自 refresh_*_token 的错误消息）
            msg.contains("凭证已过期或无效") ||
            msg.contains("权限不足") ||
            msg.contains("已被限流") ||
            msg.contains("服务器错误") ||
            msg.contains("Token 刷新失败") ||
            msg.contains("暂时不可用") ||
            // 网络错误（reqwest 错误）
            msg.contains("error trying to connect") ||
            msg.contains("connection") ||
            msg.contains("timeout") ||
            msg.contains("timed out");

        if is_upstream_error {
            AdminServiceError::UpstreamError(msg)
        } else {
            // 3. 默认归类为内部错误（本地验证失败、配置错误等）
            // 包括：缺少 refreshToken、refreshToken 已被截断、无法生成 machineId 等
            AdminServiceError::InternalError(msg)
        }
    }

    /// 分类添加凭据错误
    fn classify_add_error(&self, e: anyhow::Error) -> AdminServiceError {
        let msg = e.to_string();

        // 凭据验证失败（refreshToken 无效、格式错误等）
        let is_invalid_credential = msg.contains("缺少 refreshToken")
            || msg.contains("refreshToken 为空")
            || msg.contains("refreshToken 已被截断")
            || msg.contains("缺少 kiroApiKey")
            || msg.contains("kiroApiKey 为空")
            || msg.contains("API Key 凭据无需刷新 Token")
            || msg.contains("凭据已存在")
            || msg.contains("refreshToken 或 kiroApiKey 重复")
            || msg.contains("凭证已过期或无效")
            || msg.contains("认证失败")
            || msg.contains("权限不足")
            || msg.contains("已被限流");

        if is_invalid_credential {
            AdminServiceError::InvalidCredential(msg)
        } else if msg.contains("error trying to connect")
            || msg.contains("connection")
            || msg.contains("timeout")
        {
            AdminServiceError::UpstreamError(msg)
        } else {
            AdminServiceError::InternalError(msg)
        }
    }

    /// 分类删除凭据错误
    fn classify_delete_error(&self, e: anyhow::Error, id: u64) -> AdminServiceError {
        let msg = e.to_string();
        if msg.contains("不存在") {
            AdminServiceError::NotFound { id }
        } else if msg.contains("只能删除已禁用的凭据") || msg.contains("请先禁用凭据")
        {
            AdminServiceError::InvalidCredential(msg)
        } else {
            AdminServiceError::InternalError(msg)
        }
    }

    /// 批量导入 token.json
    ///
    /// 解析官方 token.json 格式，按 provider 字段自动映射 authMethod：
    /// - BuilderId/builder-id/idc → idc
    /// - Social/social → social
    pub async fn import_token_json(&self, req: ImportTokenJsonRequest) -> ImportTokenJsonResponse {
        let items = req.items.into_vec();
        let dry_run = req.dry_run;

        let mut results = Vec::with_capacity(items.len());
        let mut added = 0usize;
        let mut skipped = 0usize;
        let mut invalid = 0usize;

        for (index, item) in items.into_iter().enumerate() {
            let result = self.process_token_json_item(index, item, dry_run).await;
            match result.action {
                ImportAction::Added => added += 1,
                ImportAction::Skipped => skipped += 1,
                ImportAction::Invalid => invalid += 1,
            }
            results.push(result);
        }

        if !dry_run {
            let refresh_ids = results
                .iter()
                .filter(|result| result.action == ImportAction::Added)
                .filter_map(|result| result.credential_id)
                .collect::<Vec<_>>();
            self.schedule_import_usage_refresh(refresh_ids);
        }

        ImportTokenJsonResponse {
            summary: ImportSummary {
                parsed: results.len(),
                added,
                skipped,
                invalid,
            },
            items: results,
        }
    }

    /// 处理单个 token.json 项
    async fn process_token_json_item(
        &self,
        index: usize,
        item: TokenJsonItem,
        dry_run: bool,
    ) -> ImportItemResult {
        // 生成指纹（用于识别和去重）
        let fingerprint = Self::generate_fingerprint(&item);

        // 验证必填字段
        let refresh_token = match &item.refresh_token {
            Some(rt) if !rt.is_empty() => rt.clone(),
            _ => {
                return ImportItemResult {
                    index,
                    fingerprint,
                    action: ImportAction::Invalid,
                    reason: Some("缺少 refreshToken".to_string()),
                    credential_id: None,
                };
            }
        };

        // 映射 authMethod
        let auth_method = Self::map_auth_method(&item);

        // IdC 需要 clientId 和 clientSecret
        if auth_method == "idc" && (item.client_id.is_none() || item.client_secret.is_none()) {
            return ImportItemResult {
                index,
                fingerprint,
                action: ImportAction::Invalid,
                reason: Some(format!("{} 认证需要 clientId 和 clientSecret", auth_method)),
                credential_id: None,
            };
        }

        // 检查是否已存在（通过 refreshToken 前缀匹配）
        if self.token_manager.has_refresh_token_prefix(&refresh_token) {
            return ImportItemResult {
                index,
                fingerprint,
                action: ImportAction::Skipped,
                reason: Some("凭据已存在".to_string()),
                credential_id: None,
            };
        }

        // dry-run 模式只返回预览
        if dry_run {
            return ImportItemResult {
                index,
                fingerprint,
                action: ImportAction::Added,
                reason: Some("预览模式".to_string()),
                credential_id: None,
            };
        }

        // 实际添加凭据（trim + 空字符串转 None，与 set_region 逻辑一致）
        let region = item
            .region
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let api_region = item
            .api_region
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let new_cred = KiroCredentials {
            id: None,
            access_token: None,
            refresh_token: Some(refresh_token),
            kiro_api_key: None,
            profile_arn: None,
            expires_at: None,
            auth_method: Some(auth_method),
            client_id: item.client_id,
            client_secret: item.client_secret,
            priority: item.priority,
            region,
            api_region,
            machine_id: item.machine_id,
            endpoint: None,
            idp: None,
            overage_enabled: None,
            email: item.email,
            subscription_title: item.subscription_title,
            proxy_url: None,
            proxy_username: None,
            proxy_password: None,
            disabled: false,
            runtime_only: false,
        };

        match self.token_manager.add_credential(new_cred).await {
            Ok(credential_id) => ImportItemResult {
                index,
                fingerprint,
                action: ImportAction::Added,
                reason: None,
                credential_id: Some(credential_id),
            },
            Err(e) => ImportItemResult {
                index,
                fingerprint,
                action: ImportAction::Invalid,
                reason: Some(e.to_string()),
                credential_id: None,
            },
        }
    }

    fn schedule_import_usage_refresh(&self, credential_ids: Vec<u64>) {
        if credential_ids.is_empty() {
            return;
        }

        let token_manager = Arc::clone(&self.token_manager);
        tokio::spawn(async move {
            for (index, credential_id) in credential_ids.into_iter().enumerate() {
                if index > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }

                if let Err(e) = token_manager.get_usage_limits_for(credential_id).await {
                    tracing::warn!(
                        "导入凭据后后台获取订阅等级失败（不影响凭据导入）: {}",
                        e
                    );
                }
            }
        });
    }

    /// 生成凭据指纹（用于识别）
    fn generate_fingerprint(item: &TokenJsonItem) -> String {
        // 使用 refreshToken 前 16 字符作为指纹
        // 使用 floor_char_boundary 安全截断，避免在多字节字符中间切割导致 panic
        item.refresh_token
            .as_ref()
            .map(|rt| {
                if rt.len() >= 16 {
                    let end = floor_char_boundary(rt, 16);
                    format!("{}...", &rt[..end])
                } else {
                    rt.clone()
                }
            })
            .unwrap_or_else(|| "(empty)".to_string())
    }

    /// 映射 provider/authMethod 到标准 authMethod
    fn map_auth_method(item: &TokenJsonItem) -> String {
        // 优先使用 authMethod 字段
        if let Some(auth) = &item.auth_method {
            let auth_lower = auth.to_lowercase();
            return match auth_lower.as_str() {
                "idc" | "builder-id" | "builderid" => "idc".to_string(),
                "social" => "social".to_string(),
                _ => auth_lower,
            };
        }

        // 回退到 provider 字段
        if let Some(provider) = &item.provider {
            let provider_lower = provider.to_lowercase();
            return match provider_lower.as_str() {
                "builderid" | "builder-id" | "idc" => "idc".to_string(),
                "social" => "social".to_string(),
                _ => "social".to_string(), // 默认 social
            };
        }

        // 默认 social
        "social".to_string()
    }

    /// 获取当前代理配置（脱敏）
    pub fn get_proxy_config(&self) -> ProxyConfigResponse {
        let config = self.config.read();
        ProxyConfigResponse {
            proxy_url: config.proxy_url.clone(),
            has_credentials: config.proxy_username.is_some() && config.proxy_password.is_some(),
        }
    }

    /// 更新代理配置（热更新）
    pub async fn update_proxy_config(
        &self,
        req: UpdateProxyConfigRequest,
    ) -> Result<(), AdminServiceError> {
        // 1. 构建新的 ProxyConfig
        let new_proxy = if let Some(url) = &req.proxy_url {
            if url.trim().is_empty() {
                None
            } else {
                let mut proxy = ProxyConfig::new(url.trim());
                if let (Some(u), Some(p)) = (&req.proxy_username, &req.proxy_password)
                    && !u.trim().is_empty()
                    && !p.trim().is_empty()
                {
                    proxy = proxy.with_auth(u.trim(), p.trim());
                }
                // 如果未提供新认证信息，保留现有认证
                if proxy.username.is_none() {
                    let config = self.config.read();
                    if let (Some(u), Some(p)) = (&config.proxy_username, &config.proxy_password) {
                        proxy = proxy.with_auth(u, p);
                    }
                }
                Some(proxy)
            }
        } else {
            None
        };

        // 2. 先持久化配置（失败时不影响运行时状态）
        {
            let mut config = self.config.write();
            config.proxy_url = new_proxy.as_ref().map(|p| p.url.clone());
            config.proxy_username = new_proxy.as_ref().and_then(|p| p.username.clone());
            config.proxy_password = new_proxy.as_ref().and_then(|p| p.password.clone());
            config
                .save()
                .map_err(|e| AdminServiceError::InternalError(e.to_string()))?;
        }

        // 3. 持久化成功后再应用运行时变更
        if let Some(provider) = &self.kiro_provider {
            provider
                .update_global_proxy(new_proxy.clone())
                .map_err(|e| AdminServiceError::InternalError(format!("代理配置无效: {}", e)))?;
        }

        // 4. 热更新 MultiTokenManager
        self.token_manager.update_proxy(new_proxy.clone());

        // 5. 同步更新 count_tokens 通道的代理配置
        crate::token::update_proxy(new_proxy);

        Ok(())
    }

    /// 获取全局配置
    pub fn get_global_config(&self) -> super::types::GlobalConfigResponse {
        let config = self.config.read();
        let c = self.compression_config.read();
        super::types::GlobalConfigResponse {
            region: config.region.clone(),
            credential_rpm: config.credential_rpm,
            prompt_cache_ttl_seconds: config.prompt_cache_ttl_seconds,
            prompt_cache_accounting_enabled: config.prompt_cache_accounting_enabled,
            default_endpoint: config.default_endpoint.clone(),
            compression: super::types::CompressionConfigResponse {
                enabled: c.enabled,
                whitespace_compression: c.whitespace_compression,
                thinking_strategy: c.thinking_strategy.clone(),
                tool_result_max_chars: c.tool_result_max_chars,
                tool_result_head_lines: c.tool_result_head_lines,
                tool_result_tail_lines: c.tool_result_tail_lines,
                tool_use_input_max_chars: c.tool_use_input_max_chars,
                tool_description_max_chars: c.tool_description_max_chars,
                max_history_turns: c.max_history_turns,
                max_history_chars: c.max_history_chars,
                max_request_body_bytes: c.max_request_body_bytes,
            },
        }
    }

    /// 更新全局配置
    pub async fn update_global_config(
        &self,
        req: super::types::UpdateGlobalConfigRequest,
    ) -> Result<(), AdminServiceError> {
        // 1. 先持久化配置（失败时不影响运行时状态）
        {
            let mut config = self.config.write();

            if let Some(region) = &req.region {
                let trimmed = region.trim();
                if trimmed.is_empty() {
                    return Err(AdminServiceError::InvalidRequest(
                        "Region 不能为空".to_string(),
                    ));
                }
                config.region = trimmed.to_string();
            }

            if let Some(rpm) = req.credential_rpm {
                config.credential_rpm = rpm;
            }

            if let Some(ttl_seconds) = req.prompt_cache_ttl_seconds {
                if !matches!(ttl_seconds, 300 | 3600) {
                    return Err(AdminServiceError::InvalidRequest(
                        "Prompt Cache TTL 仅支持 300（5分钟）或 3600（1小时）".to_string(),
                    ));
                }
                config.prompt_cache_ttl_seconds = ttl_seconds;
            }

            if let Some(enabled) = req.prompt_cache_accounting_enabled {
                config.prompt_cache_accounting_enabled = enabled;
            }

            if let Some(ref endpoint) = req.default_endpoint {
                let trimmed = endpoint.trim();
                if trimmed.is_empty() {
                    return Err(AdminServiceError::InvalidRequest(
                        "默认 endpoint 不能为空".to_string(),
                    ));
                }
                if !self.known_endpoints.contains(trimmed) {
                    return Err(AdminServiceError::InvalidRequest(format!(
                        "未知的 endpoint: {}，可用值: {}",
                        trimmed,
                        self.known_endpoints
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                }
                config.default_endpoint = trimmed.to_string();
            }

            if let Some(c) = &req.compression {
                Self::apply_compression_fields(&mut config.compression, c);
            }

            config
                .save()
                .map_err(|e| AdminServiceError::InternalError(e.to_string()))?;
        }

        // 2. 持久化成功后再应用运行时变更
        let config = self.config.read();

        // 热更新 region
        if req.region.is_some() {
            self.token_manager.update_region(config.region.clone());
        }

        // 热更新 credential_rpm
        if req.credential_rpm.is_some() {
            self.token_manager
                .update_credential_rpm(config.credential_rpm);
        }

        // 热更新 default_endpoint
        if req.default_endpoint.is_some() {
            self.token_manager
                .update_default_endpoint(config.default_endpoint.clone());
            if let Some(provider) = &self.kiro_provider
                && let Err(e) = provider.update_default_endpoint(config.default_endpoint.clone())
            {
                tracing::warn!("热更新 KiroProvider default_endpoint 失败: {}", e);
            }
        }

        // 热更新 Prompt Cache 运行时配置
        if req.prompt_cache_ttl_seconds.is_some() || req.prompt_cache_accounting_enabled.is_some() {
            self.prompt_cache_runtime.write().update(
                req.prompt_cache_ttl_seconds,
                req.prompt_cache_accounting_enabled,
            );
        }

        // 热更新压缩配置到运行时 Arc<RwLock<CompressionConfig>>
        if let Some(c) = &req.compression {
            let mut runtime = self.compression_config.write();
            Self::apply_compression_fields(&mut runtime, c);
        }

        Ok(())
    }

    /// 将更新请求中的压缩字段应用到目标 CompressionConfig
    fn apply_compression_fields(
        target: &mut CompressionConfig,
        src: &super::types::UpdateCompressionConfigRequest,
    ) {
        if let Some(v) = src.enabled {
            target.enabled = v;
        }
        if let Some(v) = src.whitespace_compression {
            target.whitespace_compression = v;
        }
        if let Some(ref v) = src.thinking_strategy {
            target.thinking_strategy = v.clone();
        }
        if let Some(v) = src.tool_result_max_chars {
            target.tool_result_max_chars = v;
        }
        if let Some(v) = src.tool_result_head_lines {
            target.tool_result_head_lines = v;
        }
        if let Some(v) = src.tool_result_tail_lines {
            target.tool_result_tail_lines = v;
        }
        if let Some(v) = src.tool_use_input_max_chars {
            target.tool_use_input_max_chars = v;
        }
        if let Some(v) = src.tool_description_max_chars {
            target.tool_description_max_chars = v;
        }
        if let Some(v) = src.max_history_turns {
            target.max_history_turns = v;
        }
        if let Some(v) = src.max_history_chars {
            target.max_history_chars = v;
        }
        if let Some(v) = src.max_request_body_bytes {
            target.max_request_body_bytes = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anthropic::PromptCacheRuntime;
    use crate::kiro::endpoint::{CliEndpoint, IdeEndpoint, KiroEndpoint};
    use crate::kiro::model::credentials::KiroCredentials;
    use crate::kiro::provider::KiroProvider;
    use crate::kiro::token_manager::MultiTokenManager;
    use crate::model::config::{CompressionConfig, Config};
    use std::collections::HashSet;
    use std::env;
    use std::fs;

    fn create_test_service() -> AdminService {
        create_test_service_with_credentials(vec![KiroCredentials::default()])
    }

    fn create_test_service_with_credentials(credentials: Vec<KiroCredentials>) -> AdminService {
        let config_path = env::temp_dir().join(format!(
            "kiro-admin-service-test-{}-{}.json",
            std::process::id(),
            fastrand::u64(..)
        ));

        let config = Arc::new(RwLock::new(Config::load(&config_path).unwrap()));
        let compression_config = Arc::new(RwLock::new(CompressionConfig::default()));
        let prompt_cache_runtime = Arc::new(RwLock::new(PromptCacheRuntime::new(300, true)));

        let tm = Arc::new(
            MultiTokenManager::new(config.read().clone(), credentials, None, None, false).unwrap(),
        );

        let known_endpoints: HashSet<String> = vec!["ide".to_string(), "cli".to_string()]
            .into_iter()
            .collect();

        let mut endpoints: HashMap<String, Arc<dyn KiroEndpoint>> = HashMap::new();
        endpoints.insert("ide".to_string(), Arc::new(IdeEndpoint::new()));
        endpoints.insert("cli".to_string(), Arc::new(CliEndpoint::new()));
        let provider = Arc::new(KiroProvider::with_proxy(
            Arc::clone(&tm),
            None,
            endpoints,
            "ide".to_string(),
        ));

        AdminService::new(
            tm,
            Some(provider),
            config,
            compression_config,
            prompt_cache_runtime,
            known_endpoints,
        )
    }

    fn test_credential(id: u64, subscription_title: &str) -> KiroCredentials {
        KiroCredentials {
            id: Some(id),
            access_token: Some(format!("token-{id}")),
            expires_at: Some((Utc::now() + chrono::Duration::hours(1)).to_rfc3339()),
            subscription_title: Some(subscription_title.to_string()),
            ..Default::default()
        }
    }

    fn read_persisted_config(service: &AdminService) -> Config {
        let config_path = service.config.read().config_path().unwrap().to_path_buf();
        let content = fs::read_to_string(config_path).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    #[tokio::test]
    async fn test_update_global_config_default_endpoint_valid() {
        let service = create_test_service();

        let req = super::super::types::UpdateGlobalConfigRequest {
            region: None,
            credential_rpm: None,
            prompt_cache_ttl_seconds: None,
            prompt_cache_accounting_enabled: None,
            default_endpoint: Some("cli".to_string()),
            compression: None,
        };

        let result = service.update_global_config(req).await;
        assert!(result.is_ok());

        let config = service.get_global_config();
        assert_eq!(config.default_endpoint, "cli");
        assert_eq!(service.token_manager.config().default_endpoint, "cli");

        let persisted = read_persisted_config(&service);
        assert_eq!(persisted.default_endpoint, "cli");
    }

    #[tokio::test]
    async fn test_update_global_config_default_endpoint_empty_rejected() {
        let service = create_test_service();

        let req = super::super::types::UpdateGlobalConfigRequest {
            region: None,
            credential_rpm: None,
            prompt_cache_ttl_seconds: None,
            prompt_cache_accounting_enabled: None,
            default_endpoint: Some("".to_string()),
            compression: None,
        };

        let result = service.update_global_config(req).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("默认 endpoint 不能为空")
        );
    }

    #[tokio::test]
    async fn test_update_global_config_default_endpoint_whitespace_rejected() {
        let service = create_test_service();

        let req = super::super::types::UpdateGlobalConfigRequest {
            region: None,
            credential_rpm: None,
            prompt_cache_ttl_seconds: None,
            prompt_cache_accounting_enabled: None,
            default_endpoint: Some("   ".to_string()),
            compression: None,
        };

        let result = service.update_global_config(req).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("默认 endpoint 不能为空")
        );
    }

    #[tokio::test]
    async fn test_update_global_config_default_endpoint_unknown_rejected() {
        let service = create_test_service();

        let req = super::super::types::UpdateGlobalConfigRequest {
            region: None,
            credential_rpm: None,
            prompt_cache_ttl_seconds: None,
            prompt_cache_accounting_enabled: None,
            default_endpoint: Some("unknown".to_string()),
            compression: None,
        };

        let result = service.update_global_config(req).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("未知的 endpoint"));
        assert!(err_msg.contains("unknown"));
    }

    #[tokio::test]
    async fn test_update_global_config_default_endpoint_trimmed() {
        let service = create_test_service();

        let req = super::super::types::UpdateGlobalConfigRequest {
            region: None,
            credential_rpm: None,
            prompt_cache_ttl_seconds: None,
            prompt_cache_accounting_enabled: None,
            default_endpoint: Some("  cli  ".to_string()),
            compression: None,
        };

        let result = service.update_global_config(req).await;
        assert!(result.is_ok());

        let config = service.get_global_config();
        assert_eq!(config.default_endpoint, "cli");
        assert_eq!(service.token_manager.config().default_endpoint, "cli");

        let persisted = read_persisted_config(&service);
        assert_eq!(persisted.default_endpoint, "cli");
    }

    #[test]
    fn test_get_global_config_includes_default_endpoint() {
        let service = create_test_service();
        let config = service.get_global_config();
        assert_eq!(config.default_endpoint, "ide"); // Config::default() 的默认值
    }

    #[tokio::test]
    async fn oauth_start_rejects_enterprise_without_start_url() {
        let service = create_test_service();
        let result = service
            .start_oauth_login(crate::admin::oauth::OAuthStartRequest {
                provider: crate::admin::oauth::OAuthProvider::Enterprise,
                region: Some("us-east-1".to_string()),
                start_url: None,
                priority: 0,
                endpoint: None,
                proxy_url: None,
                proxy_username: None,
                proxy_password: None,
            })
            .await;
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
        let response = service
            .start_oauth_login(crate::admin::oauth::OAuthStartRequest {
                provider: crate::admin::oauth::OAuthProvider::Google,
                region: Some("us-east-1".to_string()),
                start_url: None,
                priority: 0,
                endpoint: Some("ide".to_string()),
                proxy_url: None,
                proxy_username: None,
                proxy_password: None,
            })
            .await
            .expect("start should succeed");
        assert_eq!(response.provider, crate::admin::oauth::OAuthProvider::Google);
        assert_eq!(response.completion_mode, "pasteCallbackUrl");
        assert_eq!(
            response.redirect_uri,
            crate::admin::oauth::SOCIAL_REDIRECT_URI
        );

        let status = service
            .oauth_status(&response.session_id)
            .expect("status should exist");
        assert_eq!(status.state, crate::admin::oauth::OAuthSessionState::Pending);
    }

    #[tokio::test]
    async fn oauth_cancel_removes_session() {
        let service = create_test_service();
        let response = service
            .start_oauth_login(crate::admin::oauth::OAuthStartRequest {
                provider: crate::admin::oauth::OAuthProvider::Github,
                region: Some("us-east-1".to_string()),
                start_url: None,
                priority: 0,
                endpoint: None,
                proxy_url: None,
                proxy_username: None,
                proxy_password: None,
            })
            .await
            .expect("start should succeed");

        service
            .cancel_oauth_login(&response.session_id)
            .expect("cancel should succeed");
        assert!(service.oauth_status(&response.session_id).is_err());
    }

    #[test]
    fn token_json_item_extracts_kam_account_metadata() {
        let item: TokenJsonItem = serde_json::from_value(serde_json::json!({
            "email": " user@example.com ",
            "refreshToken": "r".repeat(150),
            "usageData": {
                "subscriptionInfo": {
                    "subscriptionTitle": " KIRO PRO "
                }
            }
        }))
        .unwrap();

        assert_eq!(item.email.as_deref(), Some("user@example.com"));
        assert_eq!(item.subscription_title.as_deref(), Some("KIRO PRO"));
    }

    #[test]
    fn token_json_item_drops_empty_or_oversized_metadata() {
        let item: TokenJsonItem = serde_json::from_value(serde_json::json!({
            "email": "",
            "refreshToken": "r".repeat(150),
            "subscriptionTitle": " ".repeat(8)
        }))
        .unwrap();

        assert!(item.email.is_none());
        assert!(item.subscription_title.is_none());

        let item: TokenJsonItem = serde_json::from_value(serde_json::json!({
            "refreshToken": "r".repeat(150),
            "usageData": {
                "subscriptionInfo": {
                    "subscriptionTitle": "x".repeat(121)
                }
            }
        }))
        .unwrap();

        assert!(item.subscription_title.is_none());
    }

    #[test]
    fn get_all_credentials_exposes_global_and_per_credential_model_ids() {
        let mut disabled_pro = test_credential(4, "KIRO PRO");
        disabled_pro.disabled = true;
        let service = create_test_service_with_credentials(vec![
            test_credential(1, "KIRO FREE"),
            test_credential(2, "KIRO PRO"),
            test_credential(3, "TEST_INVALID_PLAN"),
            disabled_pro,
        ]);

        let response = service.get_all_credentials();
        assert!(response
            .available_model_ids
            .contains(&"claude-sonnet-5".to_string()));
        assert!(response
            .available_model_ids
            .contains(&"claude-sonnet-4-5-20250929".to_string()));

        let free = response
            .credentials
            .iter()
            .find(|credential| credential.id == 1)
            .unwrap();
        assert_eq!(free.supported_model_count, 3);
        assert_eq!(
            free.supported_model_ids,
            vec![
                "claude-sonnet-4-5-20250929",
                "claude-sonnet-4-5-20250929-thinking",
                "claude-sonnet-4-5-20250929-agentic",
            ]
        );
        assert!(!free
            .supported_model_ids
            .contains(&"claude-sonnet-5".to_string()));

        let pro = response
            .credentials
            .iter()
            .find(|credential| credential.id == 2)
            .unwrap();
        assert_eq!(pro.supported_model_count, 24);
        assert!(pro.supported_model_ids.contains(&"claude-sonnet-5".to_string()));
        assert!(pro
            .supported_model_ids
            .contains(&"claude-haiku-4-5-20251001".to_string()));

        let unknown = response
            .credentials
            .iter()
            .find(|credential| credential.id == 3)
            .unwrap();
        assert_eq!(unknown.supported_model_count, 0);
        assert!(unknown.supported_model_ids.is_empty());

        let disabled_pro = response
            .credentials
            .iter()
            .find(|credential| credential.id == 4)
            .unwrap();
        assert!(disabled_pro.disabled);
        assert_eq!(disabled_pro.supported_model_count, 24);

        let value = serde_json::to_value(response).unwrap();

        assert!(
            value.get("availableModelIds").is_some(),
            "admin credentials response must expose the global available model id union"
        );

        let credential = &value["credentials"][0];
        assert!(
            credential.get("supportedModelIds").is_some(),
            "each credential must expose model ids supported by its subscription"
        );
        assert!(
            credential.get("supportedModelCount").is_some(),
            "each credential must expose a model count for compact UI display"
        );

        let mut disabled_pro = test_credential(2, "KIRO PRO");
        disabled_pro.disabled = true;
        let service = create_test_service_with_credentials(vec![
            test_credential(1, "KIRO FREE"),
            disabled_pro,
        ]);
        let response = service.get_all_credentials();
        assert_eq!(
            response.available_model_ids,
            vec![
                "claude-sonnet-4-5-20250929",
                "claude-sonnet-4-5-20250929-thinking",
                "claude-sonnet-4-5-20250929-agentic",
            ]
        );
        assert!(!response
            .available_model_ids
            .contains(&"claude-sonnet-5".to_string()));
    }
}
