# Admin OAuth Account Login Design

## Context

`kiro-rs` already supports credential persistence and refresh once a complete credential exists. Admin UI can manually add or import credentials with `refreshToken`, `clientId`, `clientSecret`, `region`, and related metadata, then the existing `MultiTokenManager` validates and persists them.

The missing capability is starting account authorization from Admin UI for users who do not already have exported credentials. The reference implementation is `hj01857655/kiro-account-manager` (KAM), but KAM is a Tauri desktop app. Its OAuth callbacks depend on desktop-only behavior:

- Social providers (`Google`, `Github`) use a custom deep link redirect URI: `kiro://kiro.kiroAgent/authenticate-success`.
- IdC providers (`BuilderId`, `Enterprise`) use AWS SSO OIDC with a loopback callback at `http://127.0.0.1:<port>/oauth/callback`.

`kiro-rs` runs as a web service and often runs in Docker, accessed from LAN. A browser callback to `127.0.0.1` points to the browser device, not the Docker container, and Social web callbacks such as `http://127.0.0.1:8990/...` or LAN HTTP callbacks are rejected by Cognito as `redirect_mismatch`.

## Goals

- Add an Admin UI OAuth login flow that can create usable `KiroCredentials` without requiring users to export `token.json`.
- Support `Google`, `Github`, `BuilderId`, and `Enterprise`.
- Make pasted callback URLs the primary flow: users paste the full callback URL, and the app parses `code` and `state`.
- Reuse the existing credential validation, token refresh, usage refresh, model availability, and persistence behavior.
- Keep manual refresh-token add/import workflows unchanged.
- Preserve Admin API authentication requirements for all OAuth orchestration endpoints.

## Non-Goals

- Do not build a public OAuth callback service.
- Do not require users to configure public DNS, HTTPS, or reverse proxies.
- Do not attempt Social HTTP callback support because Cognito rejects unregistered web redirect URIs.
- Do not replace `KiroCredentials` or the existing token manager.
- Do not store extra KAM-only metadata unless it is required for refresh or current Admin display.

## Providers

### Social: Google and Github

The backend creates an OAuth session with:

- `provider`: `Google` or `Github`
- `authMethod`: `social`
- `redirectUri`: `kiro://kiro.kiroAgent/authenticate-success`
- `state`: random string generated from OS randomness
- `codeVerifier`: PKCE verifier
- `codeChallenge`: `base64url(sha256(codeVerifier))`
- `machineId`: generated per login session

The auth URL uses the KAM/Kiro desktop endpoint:

```text
https://prod.us-east-1.auth.desktop.kiro.dev/login?idp=<provider>&redirect_uri=<encoded redirectUri>&code_challenge=<challenge>&code_challenge_method=S256&state=<state>
```

The user completes browser authorization. The browser then tries to open the `kiro://...` deep link. In Admin UI, the user pastes the full callback URL, for example:

```text
kiro://kiro.kiroAgent/authenticate-success?code=...&state=...
```

The backend parses the pasted URL, verifies `state`, and exchanges the code with:

```text
POST https://prod.us-east-1.auth.desktop.kiro.dev/oauth/token
```

Request body:

```json
{
  "code": "...",
  "code_verifier": "...",
  "redirect_uri": "kiro://kiro.kiroAgent/authenticate-success"
}
```

The response is mapped into `KiroCredentials`:

- `access_token` from `accessToken`
- `refresh_token` from `refreshToken`
- `profile_arn` from `profileArn`
- `expires_at` from `expiresIn`
- `auth_method = "social"`
- `machine_id` from the session
- `idp = "Google"` or `"Github"`

After adding the credential, Admin service triggers the existing usage lookup so `email`, `subscription_title`, and model availability become visible.

### IdC: BuilderId and Enterprise

The backend creates an OAuth session with:

- `provider`: `BuilderId` or `Enterprise`
- `authMethod`: `idc`
- `region`: default `us-east-1`, user-configurable
- `startUrl`: default `https://view.awsapps.com/start` for BuilderId; required user input for Enterprise
- `state`: random string generated from OS randomness
- `codeVerifier`: PKCE verifier
- `codeChallenge`: `base64url(sha256(codeVerifier))`

The backend registers an AWS SSO OIDC client:

```text
POST https://oidc.<region>.amazonaws.com/client/register
```

Request body follows KAM:

```json
{
  "clientName": "Kiro IDE",
  "clientType": "public",
  "scopes": [
    "codewhisperer:completions",
    "codewhisperer:analysis",
    "codewhisperer:conversations",
    "codewhisperer:transformations",
    "codewhisperer:taskassist"
  ],
  "grantTypes": ["authorization_code", "refresh_token"],
  "redirectUris": ["http://127.0.0.1/oauth/callback"],
  "issuerUrl": "<startUrl>"
}
```

The session then produces an authorize URL using a loopback redirect URI:

```text
http://127.0.0.1:<port>/oauth/callback
```

For a web service, the primary completion path is still pasted callback URL. The UI asks the user to paste the final callback URL or the browser address containing `code` and `state`. Same-host automatic callback can be added as an enhancement by temporarily listening on the loopback port, but the flow must not depend on it because Docker/LAN usage breaks loopback assumptions.

The backend exchanges the code with:

```text
POST https://oidc.<region>.amazonaws.com/token
```

Request body:

```json
{
  "clientId": "...",
  "clientSecret": "...",
  "grantType": "authorization_code",
  "code": "...",
  "codeVerifier": "...",
  "redirectUri": "http://127.0.0.1:<port>/oauth/callback"
}
```

The response is mapped into `KiroCredentials`:

- `access_token` from `accessToken`
- `refresh_token` from `refreshToken`
- `expires_at` from `expiresIn`
- `auth_method = "idc"`
- `client_id` from client registration
- `client_secret` from client registration
- `region` from the session
- `machine_id` generated per login session
- `profile_arn = None`

After adding the credential, Admin service triggers the existing usage lookup so `email`, `subscription_title`, and model availability become visible when upstream data is available.

## Admin API Design

All endpoints live under `/api/admin/oauth` and require the existing Admin API key.

### `POST /api/admin/oauth/start`

Starts a login session and returns an auth URL.

Request:

```json
{
  "provider": "Google",
  "region": "us-east-1",
  "startUrl": null,
  "priority": 0,
  "endpoint": null,
  "proxyUrl": null,
  "proxyUsername": null,
  "proxyPassword": null
}
```

Response:

```json
{
  "sessionId": "...",
  "provider": "Google",
  "authMethod": "social",
  "authUrl": "https://...",
  "redirectUri": "kiro://kiro.kiroAgent/authenticate-success",
  "expiresAt": "2026-07-05T03:00:00Z",
  "completionMode": "pasteCallbackUrl"
}
```

Validation:

- `provider` must be one of `Google`, `Github`, `BuilderId`, or `Enterprise`.
- `Enterprise` requires a valid `https://...` `startUrl`.
- `region` defaults to `us-east-1` and must be non-empty.
- `endpoint`, when supplied, must be one of the registered endpoints.

### `POST /api/admin/oauth/complete`

Completes a login session from a pasted callback URL.

Request:

```json
{
  "sessionId": "...",
  "callbackUrl": "kiro://kiro.kiroAgent/authenticate-success?code=...&state=..."
}
```

The backend accepts a full callback URL only. It parses `code` and `state` from that URL server-side.

Response:

```json
{
  "success": true,
  "credentialId": 2,
  "email": "user@example.com",
  "subscriptionTitle": "KIRO FREE",
  "supportedModelIds": ["claude-sonnet-4-5-20250929"]
}
```

Errors:

- Unknown or expired session
- Missing `code`
- Missing or mismatched `state`
- OAuth provider returned an error
- Token exchange failed
- Duplicate credential
- Credential validation failed

### `GET /api/admin/oauth/status/{sessionId}`

Returns session state for UI polling or recovery.

States:

- `pending`
- `completed`
- `failed`
- `cancelled`
- `expired`

Recoverable pasted-input errors such as a missing callback URL, malformed callback URL, or mismatched `state` keep the session `pending` with an `error` message so the user can paste again. Token exchange and credential persistence failures move the session to `failed`.

### `POST /api/admin/oauth/cancel/{sessionId}`

Cancels and removes a pending session.

## Backend Components

### `src/admin/oauth.rs`

New focused module for OAuth orchestration:

- Provider enum and request/response types
- PKCE generation
- Callback URL parsing
- Social auth URL construction
- Social token exchange
- IdC client registration
- IdC auth URL construction
- IdC token exchange
- Credential mapping

### Session Store

Admin service holds an in-memory session store:

- Keyed by `sessionId`
- Stores `provider`, `authMethod`, `state`, `codeVerifier`, `redirectUri`, `region`, `startUrl`, `clientId`, `clientSecret`, `machineId`, priority and credential options
- Session TTL: 10 minutes
- Expired sessions are removed opportunistically on start/status/complete/cancel
- After successful completion, sensitive fields (`codeVerifier`, `clientSecret`, tokens, callback URL, authorization code) are cleared immediately. A sanitized completed state with `credentialId` may remain until the session TTL so the UI can recover from a refresh.

The store uses `parking_lot::Mutex<HashMap<String, OAuthSession>>`, matching existing local locking style. PKCE and state generation use OS randomness; add a small randomness dependency such as `getrandom` if the existing dependency set cannot provide that safely.

## Frontend UX

Admin UI adds an OAuth login option near "添加凭据" and "导入凭据".

Flow:

1. User selects provider.
2. If Enterprise, user enters `startUrl` and region.
3. UI calls `/api/admin/oauth/start`.
4. UI opens `authUrl` in a new tab or provides a copy button if pop-up is blocked.
5. UI shows a paste box for the callback URL.
6. User pastes the full callback URL.
7. UI calls `/api/admin/oauth/complete`.
8. On success, UI invalidates credentials and cached balances queries.
9. New credential appears with subscription and available models after existing refresh/usage logic runs.

The UI text should not promise a fully automatic callback. It should state that the expected completion input is the callback URL after login.

## Security

- All OAuth endpoints require Admin API key.
- `state` must be generated server-side and verified on completion.
- `codeVerifier` stays server-side only.
- Tokens and client secrets must never be returned to the frontend.
- Logs must not print callback URLs, codes, access tokens, refresh tokens, or client secrets.
- Cancelled, failed, and expired sessions are removed from memory. Completed sessions retain only sanitized result metadata until TTL.
- Only one completion attempt should mutate credentials for a session.

## Error Handling

User-facing errors should be specific enough to fix the flow:

- "回调 URL 缺少 code"
- "state 不匹配，请重新开始登录"
- "登录会话已过期，请重新开始"
- "Enterprise 需要填写 Start URL"
- "AWS SSO client 注册失败，请检查 region/startUrl"
- "Token 交换失败，请重新授权"
- "凭据已存在"

Backend errors should retain detailed context internally without leaking secrets.

## Testing

Backend unit tests:

- PKCE verifier/challenge generation produces non-empty URL-safe strings.
- Callback parser extracts `code` and `state` from `kiro://...`.
- Callback parser extracts `code` and `state` from `http://127.0.0.1:<port>/oauth/callback?...`.
- Callback parser rejects missing `code`.
- Callback parser rejects mismatched `state`.
- Social credential mapper fills `auth_method`, `refresh_token`, `access_token`, `profile_arn`, `expires_at`, `machine_id`, and `idp`.
- IdC credential mapper fills `auth_method`, `refresh_token`, `access_token`, `client_id`, `client_secret`, `region`, `expires_at`, and `machine_id`.
- Session store expires old sessions.

Frontend tests:

- OAuth dialog validates Enterprise `startUrl`.
- OAuth dialog opens or exposes `authUrl`.
- OAuth dialog submits pasted callback URL.
- OAuth success invalidates credentials query.
- OAuth failure shows the backend error message.

Manual verification:

- Social start URL uses `kiro://kiro.kiroAgent/authenticate-success`.
- Pasting a valid Social callback URL creates a credential.
- Pasting a callback URL with wrong `state` fails.
- BuilderId start returns an AWS authorize URL.
- Existing manual add/import flows still work.

## Claude Code Review Notes

Claude Code reviewed the proposed design before this spec. It agreed that the main risk is redirect handling in Docker/LAN environments rather than Rust data modeling. It recommended not relying on web callbacks for Social login, using server-side PKCE sessions, preserving `state` validation, keeping `codeVerifier` server-side, cleaning up expired sessions, and mapping token-exchange results into the existing `KiroCredentials` path.

After additional local verification, Social HTTP callback URLs were confirmed to fail at Cognito with `redirect_mismatch`, while the KAM `kiro://kiro.kiroAgent/authenticate-success` redirect proceeds. This spec therefore makes pasted callback URL completion the primary flow.

## Implementation Scope

This spec intentionally keeps IdC same-host automatic loopback callback as an enhancement, not a required first implementation. The first implementation should work through pasted callback URL completion for all providers.
