# Dynamic Model Capability Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose only the union of models available from the currently enabled, recognized Kiro credentials, and route each `/v1/messages` request only to credentials that support the requested model.

**Architecture:** Move the static model list into a catalog module, add an evidence-based subscription capability table, and make both `/v1/models` and credential selection consume the same capability API. Unknown or missing subscription titles resolve to no models; they must never cause `/v1/models` to fall back to all models.

**Tech Stack:** Rust 2024 edition, axum, parking_lot, tokio, serde, existing `MultiTokenManager`, existing Anthropic/Kiro converter modules, existing in-file Rust unit tests, Claude Code CLI `claude` for review gates.

## Global Constraints

- Plans and implementation notes must be written in English.
- Do not add runtime network probing for model availability.
- Do not guess model availability beyond the official Kiro Models matrix recorded in `docs/superpowers/specs/2026-07-04-model-capability-table-research.md`.
- Unknown, missing, or unrecognized `subscription_title` maps to an empty model set.
- `/v1/models` must return the union across enabled credentials with recognized subscriptions.
- `/v1/models` must return an empty list when there are no credentials, no enabled credentials, or only unknown-subscription credentials.
- Request routing must use the same subscription/model capability table as `/v1/models`.
- Official Kiro models that this proxy cannot currently map or expose must not be returned only because Kiro supports them.
- `KIRO FREE` initially exposes only the proxy-supported Claude Sonnet 4.5 variants.
- Paid tiers (`KIRO PRO`, `KIRO PRO+`, `KIRO PRO MAX`, `KIRO POWER`) expose the proxy-supported premium Claude variants listed in the research doc.
- Run Claude Code review after Task 2, after Task 5, and after Task 7. Address concrete findings before moving on.
- The branch is `research/model-capability-table`; keep the existing draft PR `https://github.com/gaozuo/kiro.rs/pull/1`.

---

## File Structure

- Create `src/anthropic/model_catalog.rs`
  - Owns the proxy-supported `Model` catalog currently hard-coded in `src/anthropic/handlers.rs`.
  - Provides ordered model lookup and filtering helpers used by `/v1/models`.
- Create `src/kiro/model/capabilities.rs`
  - Owns subscription normalization and the subscription-to-model-id table.
  - Exposes pure helpers for unknown filtering, union calculation, and per-model checks.
- Modify `src/anthropic/mod.rs`
  - Declares `model_catalog`.
- Modify `src/anthropic/handlers.rs`
  - Makes `get_models` state-aware.
  - Uses `model_catalog` plus `KiroProvider::available_model_ids()`.
  - Captures the original request model and passes it through provider calls.
- Modify `src/anthropic/router.rs`
  - Keeps the `/models` route but relies on the new `get_models(State<AppState>, OriginalUri)` handler.
- Modify `src/kiro/model/mod.rs`
  - Declares `capabilities`.
- Modify `src/kiro/provider.rs`
  - Exposes `available_model_ids()`.
  - Accepts `requested_model` in `call_api`, `call_api_stream`, and `call_api_with_retry`.
  - Passes the requested model to `MultiTokenManager`.
- Modify `src/kiro/token_manager.rs`
  - Adds snapshot helpers for enabled credentials' available model ids.
  - Adds model-aware acquire methods while preserving existing non-model methods for other paths.
  - Ensures user affinity does not pin a request to a credential that lacks the requested model.
- Modify `README.md`
  - Updates the `/v1/models` behavior from static global list to account-union list.
  - Documents unknown-subscription filtering and model-aware routing.
- Test in existing in-file `#[cfg(test)]` modules:
  - `src/kiro/model/capabilities.rs`
  - `src/anthropic/model_catalog.rs`
  - `src/kiro/token_manager.rs`
  - `src/anthropic/handlers.rs`

---

## Task 1: Add Subscription Capability Table

**Files:**
- Create: `src/kiro/model/capabilities.rs`
- Modify: `src/kiro/model/mod.rs`
- Test: `src/kiro/model/capabilities.rs`

**Interfaces:**
- Produces: `SubscriptionTier`, `normalize_subscription_title(title: Option<&str>) -> SubscriptionTier`
- Produces: `model_ids_for_subscription(title: Option<&str>) -> &'static [&'static str]`
- Produces: `credential_supports_model(title: Option<&str>, model_id: &str) -> bool`
- Produces: `union_model_ids_for_subscriptions<I>(titles: I) -> Vec<&'static str>` where `I: IntoIterator<Item = Option<String>>`
- Consumes: The official matrix recorded in `docs/superpowers/specs/2026-07-04-model-capability-table-research.md`

- [ ] **Step 1: Write the failing capability tests**

Add `src/kiro/model/capabilities.rs` with tests first:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionTier {
    Free,
    Pro,
    ProPlus,
    ProMax,
    Power,
    Unknown,
}

pub fn normalize_subscription_title(_title: Option<&str>) -> SubscriptionTier {
    panic!("red phase: normalize_subscription_title is not implemented yet")
}

pub fn model_ids_for_subscription(_title: Option<&str>) -> &'static [&'static str] {
    panic!("red phase: model_ids_for_subscription is not implemented yet")
}

pub fn credential_supports_model(_title: Option<&str>, _model_id: &str) -> bool {
    panic!("red phase: credential_supports_model is not implemented yet")
}

pub fn union_model_ids_for_subscriptions<I>(_titles: I) -> Vec<&'static str>
where
    I: IntoIterator<Item = Option<String>>,
{
    panic!("red phase: union_model_ids_for_subscriptions is not implemented yet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_and_missing_subscription_have_no_models() {
        assert_eq!(normalize_subscription_title(None), SubscriptionTier::Unknown);
        assert!(model_ids_for_subscription(None).is_empty());
        assert!(model_ids_for_subscription(Some("KIRO STUDENT")).is_empty());
        assert!(!credential_supports_model(None, "claude-sonnet-4-5-20250929"));
    }

    #[test]
    fn free_subscription_only_exposes_sonnet_45_variants() {
        let ids = model_ids_for_subscription(Some("KIRO FREE"));
        assert_eq!(
            ids,
            &[
                "claude-sonnet-4-5-20250929",
                "claude-sonnet-4-5-20250929-thinking",
                "claude-sonnet-4-5-20250929-agentic",
            ]
        );
        assert!(credential_supports_model(
            Some("KIRO FREE"),
            "claude-sonnet-4-5-20250929"
        ));
        assert!(!credential_supports_model(
            Some("KIRO FREE"),
            "claude-haiku-4-5-20251001"
        ));
        assert!(!credential_supports_model(
            Some("KIRO FREE"),
            "claude-sonnet-5"
        ));
        assert!(!credential_supports_model(
            Some("KIRO FREE"),
            "claude-opus-4-8"
        ));
    }

    #[test]
    fn paid_subscriptions_expose_proxy_supported_premium_models() {
        for title in ["KIRO PRO", "KIRO PRO+", "KIRO PRO MAX", "KIRO POWER"] {
            assert!(credential_supports_model(Some(title), "claude-sonnet-5"));
            assert!(credential_supports_model(Some(title), "claude-sonnet-4-6"));
            assert!(credential_supports_model(Some(title), "claude-opus-4-8"));
            assert!(credential_supports_model(Some(title), "claude-haiku-4-5-20251001"));
            assert!(credential_supports_model(
                Some(title),
                "claude-sonnet-4-5-20250929"
            ));
        }
    }

    #[test]
    fn union_deduplicates_and_preserves_capability_order() {
        let ids = union_model_ids_for_subscriptions(vec![
            Some("KIRO FREE".to_string()),
            None,
            Some("KIRO PRO".to_string()),
            Some("KIRO FREE".to_string()),
        ]);
        assert_eq!(ids.first(), Some(&"claude-sonnet-5"));
        assert!(ids.contains(&"claude-sonnet-4-5-20250929"));
        assert!(ids.contains(&"claude-opus-4-8"));
        assert_eq!(
            ids.iter()
                .filter(|id| **id == "claude-sonnet-4-5-20250929")
                .count(),
            1
        );
    }

    #[test]
    fn subscription_matching_accepts_common_spacing_and_case_variants() {
        assert_eq!(
            normalize_subscription_title(Some("kiro pro plus")),
            SubscriptionTier::ProPlus
        );
        assert_eq!(
            normalize_subscription_title(Some("KIRO PRO+")),
            SubscriptionTier::ProPlus
        );
        assert_eq!(
            normalize_subscription_title(Some("Kiro Pro Max")),
            SubscriptionTier::ProMax
        );
    }
}
```

- [ ] **Step 2: Wire the module so tests compile**

Modify `src/kiro/model/mod.rs`:

```rust
pub mod available_profiles;
pub mod capabilities;
pub mod credentials;
pub mod events;
pub mod requests;
pub mod token_refresh;
pub mod usage_limits;
```

- [ ] **Step 3: Run the capability tests and verify they fail for red-phase stubs**

Run:

```bash
cargo test kiro::model::capabilities -- --nocapture
```

Expected: tests compile and fail because the functions are not implemented yet.

- [ ] **Step 4: Implement the capability table**

Replace `src/kiro/model/capabilities.rs` with:

```rust
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionTier {
    Free,
    Pro,
    ProPlus,
    ProMax,
    Power,
    Unknown,
}

const FREE_MODEL_IDS: &[&str] = &[
    "claude-sonnet-4-5-20250929",
    "claude-sonnet-4-5-20250929-thinking",
    "claude-sonnet-4-5-20250929-agentic",
];

const PAID_MODEL_IDS: &[&str] = &[
    "claude-sonnet-5",
    "claude-sonnet-5-thinking",
    "claude-sonnet-5-agentic",
    "claude-sonnet-4-6",
    "claude-sonnet-4-6-thinking",
    "claude-sonnet-4-6-agentic",
    "claude-sonnet-4-5-20250929",
    "claude-sonnet-4-5-20250929-thinking",
    "claude-sonnet-4-5-20250929-agentic",
    "claude-opus-4-5-20251101",
    "claude-opus-4-5-20251101-thinking",
    "claude-opus-4-5-20251101-agentic",
    "claude-opus-4-6",
    "claude-opus-4-6-thinking",
    "claude-opus-4-6-agentic",
    "claude-opus-4-7",
    "claude-opus-4-7-thinking",
    "claude-opus-4-7-agentic",
    "claude-opus-4-8",
    "claude-opus-4-8-thinking",
    "claude-opus-4-8-agentic",
    "claude-haiku-4-5-20251001",
    "claude-haiku-4-5-20251001-thinking",
    "claude-haiku-4-5-20251001-agentic",
];

pub fn normalize_subscription_title(title: Option<&str>) -> SubscriptionTier {
    let Some(title) = title else {
        return SubscriptionTier::Unknown;
    };
    let normalized = title
        .trim()
        .to_ascii_uppercase()
        .replace('+', " PLUS")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if normalized.contains("POWER") {
        SubscriptionTier::Power
    } else if normalized.contains("PRO MAX") {
        SubscriptionTier::ProMax
    } else if normalized.contains("PRO PLUS") {
        SubscriptionTier::ProPlus
    } else if normalized.contains("PRO") {
        SubscriptionTier::Pro
    } else if normalized.contains("FREE") {
        SubscriptionTier::Free
    } else {
        SubscriptionTier::Unknown
    }
}

pub fn model_ids_for_subscription(title: Option<&str>) -> &'static [&'static str] {
    match normalize_subscription_title(title) {
        SubscriptionTier::Free => FREE_MODEL_IDS,
        SubscriptionTier::Pro
        | SubscriptionTier::ProPlus
        | SubscriptionTier::ProMax
        | SubscriptionTier::Power => PAID_MODEL_IDS,
        SubscriptionTier::Unknown => &[],
    }
}

pub fn credential_supports_model(title: Option<&str>, model_id: &str) -> bool {
    model_ids_for_subscription(title)
        .iter()
        .any(|candidate| *candidate == model_id)
}

pub fn union_model_ids_for_subscriptions<I>(titles: I) -> Vec<&'static str>
where
    I: IntoIterator<Item = Option<String>>,
{
    let mut selected = HashSet::new();
    for title in titles {
        for model_id in model_ids_for_subscription(title.as_deref()) {
            selected.insert(*model_id);
        }
    }

    PAID_MODEL_IDS
        .iter()
        .chain(FREE_MODEL_IDS.iter())
        .copied()
        .filter(|id| selected.contains(id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_and_missing_subscription_have_no_models() {
        assert_eq!(normalize_subscription_title(None), SubscriptionTier::Unknown);
        assert!(model_ids_for_subscription(None).is_empty());
        assert!(model_ids_for_subscription(Some("KIRO STUDENT")).is_empty());
        assert!(!credential_supports_model(None, "claude-sonnet-4-5-20250929"));
    }

    #[test]
    fn free_subscription_only_exposes_sonnet_45_variants() {
        let ids = model_ids_for_subscription(Some("KIRO FREE"));
        assert_eq!(
            ids,
            &[
                "claude-sonnet-4-5-20250929",
                "claude-sonnet-4-5-20250929-thinking",
                "claude-sonnet-4-5-20250929-agentic",
            ]
        );
        assert!(credential_supports_model(
            Some("KIRO FREE"),
            "claude-sonnet-4-5-20250929"
        ));
        assert!(!credential_supports_model(
            Some("KIRO FREE"),
            "claude-haiku-4-5-20251001"
        ));
        assert!(!credential_supports_model(
            Some("KIRO FREE"),
            "claude-sonnet-5"
        ));
        assert!(!credential_supports_model(
            Some("KIRO FREE"),
            "claude-opus-4-8"
        ));
    }

    #[test]
    fn paid_subscriptions_expose_proxy_supported_premium_models() {
        for title in ["KIRO PRO", "KIRO PRO+", "KIRO PRO MAX", "KIRO POWER"] {
            assert!(credential_supports_model(Some(title), "claude-sonnet-5"));
            assert!(credential_supports_model(Some(title), "claude-sonnet-4-6"));
            assert!(credential_supports_model(Some(title), "claude-opus-4-8"));
            assert!(credential_supports_model(Some(title), "claude-haiku-4-5-20251001"));
            assert!(credential_supports_model(
                Some(title),
                "claude-sonnet-4-5-20250929"
            ));
        }
    }

    #[test]
    fn union_deduplicates_and_preserves_capability_order() {
        let ids = union_model_ids_for_subscriptions(vec![
            Some("KIRO FREE".to_string()),
            None,
            Some("KIRO PRO".to_string()),
            Some("KIRO FREE".to_string()),
        ]);
        assert_eq!(ids.first(), Some(&"claude-sonnet-5"));
        assert!(ids.contains(&"claude-sonnet-4-5-20250929"));
        assert!(ids.contains(&"claude-opus-4-8"));
        assert_eq!(
            ids.iter()
                .filter(|id| **id == "claude-sonnet-4-5-20250929")
                .count(),
            1
        );
    }

    #[test]
    fn subscription_matching_accepts_common_spacing_and_case_variants() {
        assert_eq!(
            normalize_subscription_title(Some("kiro pro plus")),
            SubscriptionTier::ProPlus
        );
        assert_eq!(
            normalize_subscription_title(Some("KIRO PRO+")),
            SubscriptionTier::ProPlus
        );
        assert_eq!(
            normalize_subscription_title(Some("Kiro Pro Max")),
            SubscriptionTier::ProMax
        );
    }
}
```

- [ ] **Step 5: Run tests and commit**

Run:

```bash
cargo test kiro::model::capabilities -- --nocapture
```

Expected: all capability tests pass.

Commit:

```bash
git add src/kiro/model/mod.rs src/kiro/model/capabilities.rs
git commit -m "feat: add subscription model capability table"
```

---

## Task 2: Extract the Model Catalog

**Files:**
- Create: `src/anthropic/model_catalog.rs`
- Modify: `src/anthropic/mod.rs`
- Modify: `src/anthropic/handlers.rs`
- Test: `src/anthropic/model_catalog.rs`

**Interfaces:**
- Consumes: `super::types::Model`
- Produces: `supported_models() -> Vec<Model>`
- Produces: `models_for_ids<I>(ids: I) -> Vec<Model>` where `I: IntoIterator<Item = impl AsRef<str>>`
- Produces: `is_supported_model_id(model_id: &str) -> bool`

- [ ] **Step 1: Write failing catalog tests**

Create `src/anthropic/model_catalog.rs` with:

```rust
use super::types::Model;

pub fn supported_models() -> Vec<Model> {
    panic!("red phase: supported_models is not implemented yet")
}

pub fn models_for_ids<I, S>(_ids: I) -> Vec<Model>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    panic!("red phase: models_for_ids is not implemented yet")
}

pub fn is_supported_model_id(_model_id: &str) -> bool {
    panic!("red phase: is_supported_model_id is not implemented yet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_current_public_models_in_order() {
        let ids: Vec<String> = supported_models().into_iter().map(|m| m.id).collect();
        assert_eq!(ids.first().map(String::as_str), Some("claude-sonnet-5"));
        assert_eq!(
            ids.last().map(String::as_str),
            Some("claude-haiku-4-5-20251001-agentic")
        );
        assert!(ids.contains(&"claude-opus-4-8".to_string()));
        assert_eq!(ids.len(), 24);
    }

    #[test]
    fn models_for_ids_filters_and_preserves_catalog_order() {
        let models = models_for_ids([
            "claude-opus-4-8",
            "unknown-model",
            "claude-sonnet-4-5-20250929",
        ]);
        let ids: Vec<String> = models.into_iter().map(|m| m.id).collect();
        assert_eq!(
            ids,
            vec![
                "claude-sonnet-4-5-20250929".to_string(),
                "claude-opus-4-8".to_string(),
            ]
        );
    }

    #[test]
    fn unsupported_official_models_are_not_in_proxy_catalog() {
        assert!(!is_supported_model_id("auto"));
        assert!(!is_supported_model_id("claude-sonnet-4.0"));
        assert!(!is_supported_model_id("qwen3-coder-next"));
    }
}
```

- [ ] **Step 2: Wire the module**

Modify `src/anthropic/mod.rs`:

```rust
mod cache_tracker;
mod compressor;
mod converter;
mod handlers;
mod middleware;
mod model_catalog;
mod router;
mod stream;
mod tool_compression;
mod truncation;
pub mod types;
mod websearch;
```

- [ ] **Step 3: Run catalog tests and verify they fail**

Run:

```bash
cargo test anthropic::model_catalog -- --nocapture
```

Expected: tests fail because catalog functions still panic with `red phase`.

- [ ] **Step 4: Implement `supported_models()` by moving the existing list**

Move the full `Vec<Model>` currently built in `src/anthropic/handlers.rs::get_models()` into `src/anthropic/model_catalog.rs::supported_models()`. Keep every existing field value and the current order exactly.

Then add:

```rust
use std::collections::HashSet;

pub fn models_for_ids<I, S>(ids: I) -> Vec<Model>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let wanted: HashSet<String> = ids
        .into_iter()
        .map(|id| id.as_ref().to_string())
        .collect();

    supported_models()
        .into_iter()
        .filter(|model| wanted.contains(&model.id))
        .collect()
}

pub fn is_supported_model_id(model_id: &str) -> bool {
    supported_models().iter().any(|model| model.id == model_id)
}
```

- [ ] **Step 5: Make `get_models()` temporarily use the extracted catalog**

Modify `src/anthropic/handlers.rs` imports:

```rust
use super::converter::{ConversionError, convert_request};
use super::middleware::AppState;
use super::model_catalog;
use super::stream::{CacheUsageBreakdown, SseEvent, StreamContext};
```

Replace the body of `get_models()` with:

```rust
pub async fn get_models(OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    tracing::info!(
        path = %uri.path(),
        "Received request"
    );

    Json(ModelsResponse {
        object: "list".to_string(),
        data: model_catalog::supported_models(),
    })
}
```

- [ ] **Step 6: Run tests and commit**

Run:

```bash
cargo test anthropic::model_catalog -- --nocapture
cargo test test_map_model_versioned_entries_from_models_endpoint -- --nocapture
```

Expected: both commands pass. The second command must still pass because it protects backward-compatible mapping for every public model id moved into `model_catalog`.

Commit:

```bash
git add src/anthropic/mod.rs src/anthropic/model_catalog.rs src/anthropic/handlers.rs
git commit -m "refactor: extract anthropic model catalog"
```

- [ ] **Step 7: Claude Code review round 1**

Run:

```bash
claude -p "Review the current git branch after Tasks 1-2. Focus on whether the extracted model catalog exactly preserves the previous /v1/models data and all fields, whether test_map_model_versioned_entries_from_models_endpoint still passes, whether the subscription capability table follows docs/superpowers/specs/2026-07-04-model-capability-table-research.md, and whether unknown subscriptions correctly map to no models. Return only actionable findings with file paths."
```

Expected: Claude Code returns no blocking findings, or concrete findings that can be addressed before Task 3.

If Claude reports a concrete issue, fix it, rerun the relevant `cargo test` command, and commit:

```bash
git add src/anthropic src/kiro/model
git commit -m "fix: address catalog capability review"
```

---

## Task 3: Expose Available Model IDs from MultiTokenManager and KiroProvider

**Files:**
- Modify: `src/kiro/token_manager.rs`
- Modify: `src/kiro/provider.rs`
- Test: `src/kiro/token_manager.rs`

**Interfaces:**
- Consumes: `crate::kiro::model::capabilities::union_model_ids_for_subscriptions`
- Produces: `MultiTokenManager::available_model_ids(&self) -> Vec<&'static str>`
- Produces: `KiroProvider::available_model_ids(&self) -> Vec<&'static str>`

- [ ] **Step 1: Write failing token manager tests**

Add tests to `src/kiro/token_manager.rs` inside the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn available_model_ids_returns_empty_without_credentials() {
    let manager = create_test_multi_manager(vec![]);
    assert!(manager.available_model_ids().is_empty());
}

#[test]
fn available_model_ids_returns_empty_for_only_unknown_subscriptions() {
    let mut unknown = create_test_credential(1);
    unknown.subscription_title = Some("KIRO STUDENT".to_string());
    let manager = create_test_multi_manager(vec![unknown]);
    assert!(manager.available_model_ids().is_empty());
}

#[test]
fn available_model_ids_uses_only_enabled_credentials() {
    let mut free = create_test_credential(1);
    free.subscription_title = Some("KIRO FREE".to_string());
    let mut pro = create_test_credential(2);
    pro.subscription_title = Some("KIRO PRO".to_string());
    let manager = create_test_multi_manager(vec![free, pro]);
    manager.set_disabled(2, true).unwrap();

    let ids = manager.available_model_ids();
    assert_eq!(
        ids,
        vec![
            "claude-sonnet-4-5-20250929",
            "claude-sonnet-4-5-20250929-thinking",
            "claude-sonnet-4-5-20250929-agentic",
        ]
    );
}

#[test]
fn available_model_ids_merges_enabled_account_capabilities() {
    let mut free = create_test_credential(1);
    free.subscription_title = Some("KIRO FREE".to_string());
    let mut pro = create_test_credential(2);
    pro.subscription_title = Some("KIRO PRO".to_string());
    let manager = create_test_multi_manager(vec![free, pro]);

    let ids = manager.available_model_ids();
    assert!(ids.contains(&"claude-sonnet-5"));
    assert!(ids.contains(&"claude-sonnet-4-5-20250929"));
    assert!(ids.contains(&"claude-opus-4-8"));
    assert!(ids.contains(&"claude-haiku-4-5-20251001"));
}
```

- [ ] **Step 2: Run tests and verify they fail because the method is missing**

Run:

```bash
cargo test available_model_ids -- --nocapture
```

Expected: compile failure mentioning `available_model_ids` is missing.

- [ ] **Step 3: Implement `MultiTokenManager::available_model_ids()`**

Add near `available_count()` in `src/kiro/token_manager.rs`:

```rust
    /// Return the union of proxy-supported model ids for enabled credentials.
    ///
    /// Unknown subscription titles intentionally contribute no models.
    pub fn available_model_ids(&self) -> Vec<&'static str> {
        let titles = {
            let entries = self.entries.lock();
            entries
                .iter()
                .filter(|entry| !entry.disabled)
                .map(|entry| entry.credentials.subscription_title.clone())
                .collect::<Vec<_>>()
        };

        crate::kiro::model::capabilities::union_model_ids_for_subscriptions(titles)
    }
```

- [ ] **Step 4: Expose through KiroProvider**

Add to `impl KiroProvider` in `src/kiro/provider.rs`:

```rust
    /// Return the union of proxy-supported model ids for enabled credentials.
    pub fn available_model_ids(&self) -> Vec<&'static str> {
        self.token_manager.available_model_ids()
    }
```

- [ ] **Step 5: Run tests and commit**

Run:

```bash
cargo test available_model_ids -- --nocapture
```

Expected: all `available_model_ids` tests pass.

Commit:

```bash
git add src/kiro/token_manager.rs src/kiro/provider.rs
git commit -m "feat: expose available model ids from credentials"
```

---

## Task 4: Make `/v1/models` Return the Enabled Credential Union

**Files:**
- Modify: `src/anthropic/handlers.rs`
- Test: `src/anthropic/handlers.rs`

**Interfaces:**
- Consumes: `KiroProvider::available_model_ids() -> Vec<&'static str>`
- Consumes: `model_catalog::models_for_ids(ids) -> Vec<Model>`
- Produces: `get_models(State(state): State<AppState>, OriginalUri(uri): OriginalUri) -> impl IntoResponse`

- [ ] **Step 1: Write pure helper tests in handlers**

Add a helper near `get_models()`:

```rust
fn models_response_for_available_ids<I, S>(ids: I) -> ModelsResponse
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    ModelsResponse {
        object: "list".to_string(),
        data: model_catalog::models_for_ids(ids),
    }
}
```

Also add a state-aware helper:

```rust
fn models_response_for_state(state: &AppState) -> ModelsResponse {
    let available_ids = state
        .kiro_provider
        .as_ref()
        .map(|provider| provider.available_model_ids())
        .unwrap_or_default();

    models_response_for_available_ids(available_ids)
}
```

Add tests inside `src/anthropic/handlers.rs` tests:

```rust
#[test]
fn models_response_for_empty_available_ids_returns_empty_list() {
    let response = models_response_for_available_ids(Vec::<&str>::new());
    assert_eq!(response.object, "list");
    assert!(response.data.is_empty());
}

#[test]
fn models_response_for_available_ids_filters_catalog_and_preserves_catalog_order() {
    let response = models_response_for_available_ids([
        "claude-opus-4-8",
        "claude-sonnet-4-5-20250929",
        "not-supported",
    ]);
    let ids: Vec<String> = response.data.into_iter().map(|model| model.id).collect();
    assert_eq!(
        ids,
        vec![
            "claude-sonnet-4-5-20250929".to_string(),
            "claude-opus-4-8".to_string(),
        ]
    );
}

#[test]
fn models_response_for_state_with_unknown_only_credential_returns_empty_list() {
    use std::sync::Arc;

    use parking_lot::RwLock;

    use crate::kiro::model::credentials::KiroCredentials;
    use crate::kiro::provider::KiroProvider;
    use crate::kiro::token_manager::MultiTokenManager;
    use crate::model::config::Config;

    use super::super::middleware::PromptCacheRuntime;

    let mut credential = KiroCredentials::default();
    credential.id = Some(1);
    credential.subscription_title = Some("KIRO STUDENT".to_string());

    let manager = MultiTokenManager::new(
        Config::default(),
        vec![credential],
        None,
        None,
        false,
    )
    .unwrap();
    let provider = Arc::new(KiroProvider::new(Arc::new(manager)));
    let prompt_cache_runtime = Arc::new(RwLock::new(PromptCacheRuntime::new(300, true)));
    let state = AppState::new("test-api-key", prompt_cache_runtime).with_kiro_provider(provider);

    let response = models_response_for_state(&state);
    assert_eq!(response.object, "list");
    assert!(response.data.is_empty());
}
```

- [ ] **Step 2: Run helper tests**

Run:

```bash
cargo test models_response_for -- --nocapture
```

Expected: tests pass once the helper is present.

- [ ] **Step 3: Change `get_models` to use state and no provider fallback**

Replace `get_models` with:

```rust
pub async fn get_models(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
) -> impl IntoResponse {
    tracing::info!(
        path = %uri.path(),
        "Received request"
    );

    Json(models_response_for_state(&state))
}
```

This intentionally returns an empty list when `kiro_provider` is missing. It must not return the full catalog.

- [ ] **Step 4: Run model handler tests and commit**

Run:

```bash
cargo test models_response_for -- --nocapture
cargo test anthropic::model_catalog -- --nocapture
```

Expected: both commands pass.

Commit:

```bash
git add src/anthropic/handlers.rs
git commit -m "feat: return account-union models"
```

---

## Task 5: Add Model-Aware Credential Selection

**Files:**
- Modify: `src/kiro/token_manager.rs`
- Test: `src/kiro/token_manager.rs`

**Interfaces:**
- Consumes: `crate::kiro::model::capabilities::credential_supports_model(title, model_id)`
- Produces: `MultiTokenManager::credential_supports_model_id(&self, credential_id: u64, model_id: &str) -> bool`
- Produces: `MultiTokenManager::available_count_for_model(&self, model_id: Option<&str>) -> usize`
- Produces: `MultiTokenManager::acquire_context_for_user_model_excluding(user_id: Option<&str>, model_id: Option<&str>, exclude_ids: &[u64]) -> anyhow::Result<CallContext>`

- [ ] **Step 1: Write tests for model-aware counting and support**

Add tests:

```rust
#[test]
fn available_count_for_model_filters_unknown_and_unsupported_credentials() {
    let mut free = create_test_credential(1);
    free.subscription_title = Some("KIRO FREE".to_string());
    let mut pro = create_test_credential(2);
    pro.subscription_title = Some("KIRO PRO".to_string());
    let mut unknown = create_test_credential(3);
    unknown.subscription_title = Some("KIRO STUDENT".to_string());
    let manager = create_test_multi_manager(vec![free, pro, unknown]);

    assert_eq!(
        manager.available_count_for_model(Some("claude-sonnet-4-5-20250929")),
        2
    );
    assert_eq!(
        manager.available_count_for_model(Some("claude-sonnet-5")),
        1
    );
    assert_eq!(
        manager.available_count_for_model(Some("claude-haiku-4-5-20251001")),
        1
    );
    assert_eq!(
        manager.available_count_for_model(Some("unknown-model")),
        0
    );
}

#[test]
fn credential_supports_model_id_uses_subscription_table() {
    let mut free = create_test_credential(1);
    free.subscription_title = Some("KIRO FREE".to_string());
    let manager = create_test_multi_manager(vec![free]);

    assert!(manager.credential_supports_model_id(1, "claude-sonnet-4-5-20250929"));
    assert!(!manager.credential_supports_model_id(1, "claude-sonnet-5"));
    assert!(!manager.credential_supports_model_id(999, "claude-sonnet-4-5-20250929"));
}
```

- [ ] **Step 2: Run tests and verify missing method failures**

Run:

```bash
cargo test available_count_for_model -- --nocapture
cargo test credential_supports_model_id -- --nocapture
```

Expected: compile failure because methods are missing.

- [ ] **Step 3: Implement support and count helpers**

Add near `available_count()`:

```rust
    pub fn credential_supports_model_id(&self, credential_id: u64, model_id: &str) -> bool {
        let entries = self.entries.lock();
        entries
            .iter()
            .find(|entry| entry.id == credential_id && !entry.disabled)
            .is_some_and(|entry| {
                crate::kiro::model::capabilities::credential_supports_model(
                    entry.credentials.subscription_title.as_deref(),
                    model_id,
                )
            })
    }

    pub fn available_count_for_model(&self, model_id: Option<&str>) -> usize {
        let entries = self.entries.lock();
        entries
            .iter()
            .filter(|entry| !entry.disabled)
            .filter(|entry| {
                model_id.is_none_or(|id| {
                    crate::kiro::model::capabilities::credential_supports_model(
                        entry.credentials.subscription_title.as_deref(),
                        id,
                    )
                })
            })
            .count()
    }
```

- [ ] **Step 4: Add model-aware acquire methods without changing existing callers yet**

Rename the current body of `acquire_context_excluding(&self, exclude_ids: &[u64])` to a new private method by changing its signature to:

```rust
    async fn acquire_context_for_model_excluding(
        &self,
        model_id: Option<&str>,
        exclude_ids: &[u64],
    ) -> anyhow::Result<CallContext> {
```

Then add this wrapper where the old public method was:

```rust
    pub async fn acquire_context_excluding(
        &self,
        exclude_ids: &[u64],
    ) -> anyhow::Result<CallContext> {
        self.acquire_context_for_model_excluding(None, exclude_ids).await
    }
```

Inside the renamed `acquire_context_for_model_excluding` body, make these exact edits.

Replace the existing enabled-count line:

```rust
            let enabled_total = self.available_count();
```

with:

```rust
            let enabled_total = self.available_count_for_model(model_id);
            if enabled_total == 0 {
                if let Some(model_id) = model_id {
                    anyhow::bail!("没有支持模型 {} 的可用凭据", model_id);
                }
                anyhow::bail!("没有可用的凭据");
            }
```

Replace the first final token failure message:

```rust
                anyhow::bail!(
                    "所有可用凭据均无法获取有效 Token（可用: {}/{}）",
                    enabled_total,
                    total
                );
```

with:

```rust
                if let Some(model_id) = model_id {
                    anyhow::bail!(
                        "所有支持模型 {} 的可用凭据均无法获取有效 Token（可用: {}/{}）",
                        model_id,
                        enabled_total,
                        total
                    );
                }
                anyhow::bail!(
                    "所有可用凭据均无法获取有效 Token（可用: {}/{}）",
                    enabled_total,
                    total
                );
```

Replace the second exhaustion condition:

```rust
            if tried_ids.len() >= total {
```

with:

```rust
            if tried_ids.len() >= enabled_total {
```

Replace the second final token failure message:

```rust
                anyhow::bail!(
                    "所有凭据均无法获取有效 Token（可用: {}/{}）",
                    self.available_count(),
                    total
                );
```

with:

```rust
                if let Some(model_id) = model_id {
                    anyhow::bail!(
                        "所有支持模型 {} 的可用凭据均无法获取有效 Token（可用: {}/{}）",
                        model_id,
                        enabled_total,
                        total
                    );
                }
                anyhow::bail!(
                    "所有凭据均无法获取有效 Token（可用: {}/{}）",
                    self.available_count(),
                    total
                );
```

In the first `candidate_infos` construction, replace:

```rust
                    .filter(|e| !e.disabled && !tried_ids.contains(&e.id))
```

with:

```rust
                    .filter(|e| !e.disabled && !tried_ids.contains(&e.id))
                    .filter(|e| {
                        model_id.is_none_or(|id| {
                            crate::kiro::model::capabilities::credential_supports_model(
                                e.credentials.subscription_title.as_deref(),
                                id,
                            )
                        })
                    })
```

In the auto-heal rebuilt `candidates = entries.iter()` block, make the same filter replacement.

In the `if candidates.is_empty()` block, replace:

```rust
                    let available = entries.iter().filter(|e| !e.disabled).count();
```

with:

```rust
                    let available = entries
                        .iter()
                        .filter(|e| !e.disabled)
                        .filter(|e| {
                            model_id.is_none_or(|id| {
                                crate::kiro::model::capabilities::credential_supports_model(
                                    e.credentials.subscription_title.as_deref(),
                                    id,
                                )
                            })
                        })
                        .count();
```

Then replace the `available == 0` bail inside that block with:

```rust
                    if available == 0 {
                        if let Some(model_id) = model_id {
                            anyhow::bail!("没有支持模型 {} 的可用凭据", model_id);
                        }
                        anyhow::bail!("所有凭据均已禁用（{}/{}）", available, total);
                    }
```

- [ ] **Step 5: Add model-aware user affinity method**

Add:

```rust
    pub async fn acquire_context_for_user_model_excluding(
        &self,
        user_id: Option<&str>,
        model_id: Option<&str>,
        exclude_ids: &[u64],
    ) -> anyhow::Result<CallContext> {
        let user_id = match user_id {
            Some(id) if !id.is_empty() => id,
            _ => return self.acquire_context_for_model_excluding(model_id, exclude_ids).await,
        };

        let mut keep_affinity_binding = false;

        if let Some(bound_id) = self.affinity.get(user_id) {
            let bound_excluded = exclude_ids.contains(&bound_id);
            let model_supported = model_id
                .map(|id| self.credential_supports_model_id(bound_id, id))
                .unwrap_or(true);
            let is_enabled = !bound_excluded
                && model_supported
                && {
                    let entries = self.entries.lock();
                    entries.iter().any(|entry| entry.id == bound_id && !entry.disabled)
                };

            if is_enabled {
                if let Some((reason, remaining)) = self.cooldown_manager.check_cooldown(bound_id) {
                    keep_affinity_binding = matches!(
                        reason,
                        CooldownReason::RateLimitExceeded
                            | CooldownReason::TokenRefreshFailed
                            | CooldownReason::ServerError
                            | CooldownReason::ModelUnavailable
                    );
                    tracing::debug!(
                        user_id = %user_id,
                        credential_id = %bound_id,
                        reason = ?reason,
                        remaining_ms = %remaining.as_millis(),
                        keep_affinity_binding = %keep_affinity_binding,
                        "亲和性绑定凭据处于冷却，本次将分流"
                    );
                } else if let Err(wait) = self.rate_limiter.check_rate_limit(bound_id) {
                    keep_affinity_binding = true;
                    tracing::info!(
                        user_id = %mask_user_id(Some(user_id)),
                        credential_id = %bound_id,
                        wait_ms = %wait.as_millis(),
                        "亲和性绑定凭据触发速率限制，本次将分流"
                    );
                } else if let Err(wait) = self.rate_limiter.try_acquire(bound_id) {
                    keep_affinity_binding = true;
                    tracing::debug!(
                        user_id = %mask_user_id(Some(user_id)),
                        credential_id = %bound_id,
                        wait_ms = %wait.as_millis(),
                        "亲和性凭据 try_acquire 竞争失败，本次将分流"
                    );
                } else {
                    let credentials = {
                        let entries = self.entries.lock();
                        entries
                            .iter()
                            .find(|entry| entry.id == bound_id)
                            .map(|entry| entry.credentials.clone())
                    };

                    if let Some(creds) = credentials {
                        match self.try_ensure_token(bound_id, &creds).await {
                            Ok(ctx) => {
                                self.affinity.touch(user_id);
                                return Ok(ctx);
                            }
                            Err(error) => {
                                tracing::debug!(
                                    user_id = %user_id,
                                    credential_id = %bound_id,
                                    error = %error,
                                    "亲和性绑定凭据 token 获取/刷新失败，本次将分流"
                                );
                            }
                        }
                    }
                }
            } else if model_id.is_some() && !model_supported {
                tracing::debug!(
                    user_id = %user_id,
                    credential_id = %bound_id,
                    model = ?model_id,
                    "亲和性绑定凭据不支持请求模型，本次将分流"
                );
            }
        }

        let ctx = self
            .acquire_context_for_model_excluding(model_id, exclude_ids)
            .await?;
        if !keep_affinity_binding {
            self.affinity.set(user_id, ctx.id);
        }
        Ok(ctx)
    }
```

Then keep existing `acquire_context_for_user_excluding` as a compatibility wrapper:

```rust
    pub async fn acquire_context_for_user_excluding(
        &self,
        user_id: Option<&str>,
        exclude_ids: &[u64],
    ) -> anyhow::Result<CallContext> {
        self.acquire_context_for_user_model_excluding(user_id, None, exclude_ids)
            .await
    }
```

- [ ] **Step 6: Add acquire behavior tests**

Add async tests:

```rust
#[tokio::test]
async fn acquire_context_for_user_model_skips_free_for_sonnet_5() {
    let mut free = create_test_credential(1);
    free.subscription_title = Some("KIRO FREE".to_string());
    let mut pro = create_test_credential(2);
    pro.subscription_title = Some("KIRO PRO".to_string());
    let manager = create_test_multi_manager(vec![free, pro]);

    let ctx = manager
        .acquire_context_for_user_model_excluding(
            Some("user-a"),
            Some("claude-sonnet-5"),
            &[],
        )
        .await
        .unwrap();

    assert_eq!(ctx.id, 2);
}

#[tokio::test]
async fn acquire_context_for_user_model_rejects_unknown_only_pool() {
    let mut unknown = create_test_credential(1);
    unknown.subscription_title = Some("KIRO STUDENT".to_string());
    let manager = create_test_multi_manager(vec![unknown]);

    let err = manager
        .acquire_context_for_user_model_excluding(
            Some("user-a"),
            Some("claude-sonnet-5"),
            &[],
        )
        .await
        .err()
        .unwrap()
        .to_string();

    assert!(err.contains("没有支持模型 claude-sonnet-5 的可用凭据"));
}

#[tokio::test]
async fn affinity_does_not_reuse_bound_credential_for_unsupported_model() {
    let mut free = create_test_credential(1);
    free.subscription_title = Some("KIRO FREE".to_string());
    let mut pro = create_test_credential(2);
    pro.subscription_title = Some("KIRO PRO".to_string());
    let manager = create_test_multi_manager(vec![free, pro]);

    let sonnet_45 = manager
        .acquire_context_for_user_model_excluding(
            Some("user-a"),
            Some("claude-sonnet-4-5-20250929"),
            &[],
        )
        .await
        .unwrap();
    assert_eq!(sonnet_45.id, 1);

    let sonnet_5 = manager
        .acquire_context_for_user_model_excluding(
            Some("user-a"),
            Some("claude-sonnet-5"),
            &[],
        )
        .await
        .unwrap();
    assert_eq!(sonnet_5.id, 2);
}
```

- [ ] **Step 7: Run tests and commit**

Run:

```bash
cargo test available_count_for_model -- --nocapture
cargo test credential_supports_model_id -- --nocapture
cargo test acquire_context_for_user_model -- --nocapture
```

Expected: all targeted tests pass.

Commit:

```bash
git add src/kiro/token_manager.rs
git commit -m "feat: select credentials by requested model"
```

- [ ] **Step 8: Claude Code review round 2**

Run:

```bash
claude -p "Review the current git branch after Tasks 3-5. Focus on model-aware credential selection, user affinity bypass for unsupported models, preservation of retry/cooldown/rate-limit behavior, and the requirement that unknown subscriptions expose no models. Return only actionable findings with file paths."
```

Expected: Claude Code returns no blocking findings, or concrete findings that can be addressed before Task 6.

If Claude reports a concrete issue, fix it, rerun the targeted tests, and commit:

```bash
git add src/kiro/token_manager.rs src/kiro/provider.rs
git commit -m "fix: address model routing review"
```

---

## Task 6: Pass Requested Model Through Handler and Provider

**Files:**
- Modify: `src/anthropic/handlers.rs`
- Modify: `src/kiro/provider.rs`
- Test: `src/kiro/provider.rs` or compile-level targeted tests

**Interfaces:**
- Consumes: `payload.model` before conversion.
- Consumes: `KiroProvider::call_api(request_body, user_id, requested_model)`
- Consumes: `KiroProvider::call_api_stream(request_body, user_id, requested_model)`
- Produces: provider retry path calling `acquire_context_for_user_model_excluding(user_id, requested_model, failed_ids)`

- [ ] **Step 1: Update provider method signatures**

Modify `src/kiro/provider.rs`:

```rust
    pub async fn call_api(
        &self,
        request_body: &str,
        user_id: Option<&str>,
        requested_model: Option<&str>,
    ) -> anyhow::Result<ApiCallResult> {
        self.call_api_with_retry(request_body, false, user_id, requested_model)
            .await
    }

    pub async fn call_api_stream(
        &self,
        request_body: &str,
        user_id: Option<&str>,
        requested_model: Option<&str>,
    ) -> anyhow::Result<ApiCallResult> {
        self.call_api_with_retry(request_body, true, user_id, requested_model)
            .await
    }

    async fn call_api_with_retry(
        &self,
        request_body: &str,
        is_stream: bool,
        user_id: Option<&str>,
        requested_model: Option<&str>,
    ) -> anyhow::Result<ApiCallResult> {
```

Inside `call_api_with_retry`, change the available count:

```rust
        let available = self.token_manager.available_count_for_model(requested_model);
        if available == 0 {
            if let Some(model) = requested_model {
                anyhow::bail!("没有支持模型 {} 的可用凭据", model);
            }
            anyhow::bail!("没有可用的凭据");
        }
```

Change context acquisition:

```rust
            let ctx = match self
                .token_manager
                .acquire_context_for_user_model_excluding(user_id, requested_model, &failed_ids)
                .await
```

Change retry reset logic:

```rust
                    let available_for_model = self.token_manager.available_count_for_model(requested_model);
                    if available_for_model == 0 {
                        if let Some(model) = requested_model {
                            anyhow::bail!("没有支持模型 {} 的可用凭据", model);
                        }
                        anyhow::bail!("没有可用的凭据");
                    }
                    if failed_ids.len() >= available_for_model {
                        failed_ids.clear();
                    }
```

- [ ] **Step 2: Update handler call sites**

In `src/anthropic/handlers.rs`, capture the original Anthropic-compatible model id after `override_thinking_from_model_name(&mut payload)`. This value must remain the public model id used by the capability table, such as `claude-sonnet-5`, `claude-sonnet-5-thinking`, or `claude-opus-4-8`; do not pass the converted Kiro model id such as `claude-sonnet-4.5`.

```rust
    let requested_model = payload.model.clone();
```

When calling the provider for non-stream requests, pass:

```rust
provider
    .call_api(
        ctx.request_body,
        ctx.user_id,
        Some(ctx.model),
    )
    .await
```

When calling the provider for stream requests, pass:

```rust
provider
    .call_api_stream(
        ctx.request_body,
        ctx.user_id,
        Some(ctx.model),
    )
    .await
```

Ensure `StreamRequestContext` and `NonStreamRequestContext` already use `model: &'a str`; pass `requested_model.as_str()` when constructing these contexts:

```rust
model: requested_model.as_str(),
```

Add this assertion near context construction in tests or in a small helper test:

```rust
#[test]
fn request_context_model_uses_public_model_id_for_routing() {
    let requested_model = "claude-sonnet-5-thinking".to_string();
    assert!(crate::kiro::model::capabilities::credential_supports_model(
        Some("KIRO PRO"),
        requested_model.as_str()
    ));
    assert!(!crate::kiro::model::capabilities::credential_supports_model(
        Some("KIRO FREE"),
        requested_model.as_str()
    ));
}
```

- [ ] **Step 3: Update any remaining provider call sites**

Run:

```bash
rg -n "call_api\\(|call_api_stream\\(|call_api_with_retry\\(" src
```

Every `call_api` and `call_api_stream` call must pass the new `requested_model` argument. MCP paths must remain unchanged.

- [ ] **Step 4: Run compile-targeted tests**

Run:

```bash
cargo test anthropic::handlers::tests::test_override_thinking_from_model_name_sonnet_5 -- --nocapture
cargo test acquire_context_for_user_model -- --nocapture
```

Expected: tests compile and pass.

Commit:

```bash
git add src/anthropic/handlers.rs src/kiro/provider.rs
git commit -m "feat: route requests by requested model"
```

---

## Task 7: Documentation, Full Verification, and Final Review

**Files:**
- Modify: `README.md`

**Interfaces:**
- Documents: `/v1/models` dynamic union behavior.
- Documents: unknown subscription filtering.
- Documents: request-time model-aware routing.

- [ ] **Step 1: Update README model section**

Replace the existing static-list wording in `README.md` model/admin sections with:

```markdown
`GET /v1/models` returns the union of model IDs supported by the currently enabled credentials with recognized Kiro subscription titles. It does not fall back to the full proxy catalog when there are no credentials, no enabled credentials, or only unknown subscription titles.

Unknown subscription titles are treated as no model access until the account balance/usage lookup records a recognized title such as `KIRO FREE`, `KIRO PRO`, `KIRO PRO+`, `KIRO PRO MAX`, or `KIRO POWER`.

The request path uses the same capability table as `/v1/models`: a request for `claude-sonnet-5` or an Opus model will not be routed to a Free credential.
```

- [ ] **Step 2: Run targeted Rust tests**

Run:

```bash
cargo test kiro::model::capabilities -- --nocapture
cargo test anthropic::model_catalog -- --nocapture
cargo test available_model_ids -- --nocapture
cargo test available_count_for_model -- --nocapture
cargo test credential_supports_model_id -- --nocapture
cargo test acquire_context_for_user_model -- --nocapture
cargo test models_response_for -- --nocapture
```

Expected: all targeted tests pass.

- [ ] **Step 3: Run broader verification**

Run:

```bash
cargo test
```

Expected: all tests pass, or any failure is documented with the exact failing test name and whether it predates this branch. Do not claim full pass unless the command exits 0.

- [ ] **Step 4: Run Claude Code review round 3**

Run:

```bash
claude -p "Final review this branch for the dynamic model capability routing feature. Requirements: /v1/models must expose only the union across enabled recognized credentials, no credentials must not return the full catalog, unknown subscriptions must expose no models, and /v1/messages must route only to credentials supporting the requested model. Review tests and docs too. Return only actionable findings with file paths."
```

Expected: Claude Code returns no blocking findings.

If Claude reports a concrete issue, fix it, rerun the relevant targeted tests and `cargo test`, then run the same Claude command again. Commit fixes:

```bash
git add README.md src docs
git commit -m "fix: address final model routing review"
```

- [ ] **Step 5: Commit docs**

Commit:

```bash
git add README.md docs/superpowers/specs/2026-07-04-model-capability-table-research.md
git commit -m "docs: describe dynamic model availability"
```

- [ ] **Step 6: Push the implementation branch**

Run:

```bash
git push gaozuo research/model-capability-table
```

Expected: push succeeds and updates PR #1.

- [ ] **Step 7: Update PR description**

Run:

```bash
gh pr edit 1 --repo gaozuo/kiro.rs --body-file /tmp/model-capability-pr-body.md
```

Use this PR body:

```markdown
## Purpose

Implement subscription-based model availability and model-aware request routing.

## Behavior

- `/v1/models` returns only the union of models available from enabled credentials with recognized Kiro subscription titles.
- No credentials, no enabled credentials, or only unknown subscription titles return an empty model list.
- Unknown subscription titles are filtered out rather than guessed.
- `/v1/messages` uses the same capability table to choose credentials for the requested model.

## Evidence

- Official Kiro Models matrix: https://kiro.dev/docs/models/
- Research doc: `docs/superpowers/specs/2026-07-04-model-capability-table-research.md`
- Implementation plan: `docs/superpowers/plans/2026-07-04-dynamic-model-capability-routing.md`

## Review gates

- Claude Code review after catalog/capability table.
- Claude Code review after model-aware selection.
- Claude Code final review after docs and verification.

## Verification

- `cargo test kiro::model::capabilities -- --nocapture`
- `cargo test anthropic::model_catalog -- --nocapture`
- `cargo test available_model_ids -- --nocapture`
- `cargo test available_count_for_model -- --nocapture`
- `cargo test credential_supports_model_id -- --nocapture`
- `cargo test acquire_context_for_user_model -- --nocapture`
- `cargo test models_response_for -- --nocapture`
- `cargo test`
```

---

## Self-Review Checklist

- [x] Plan-authoring Claude Code review completed once, actionable findings were applied, and Claude Code re-review returned: `No remaining actionable findings.`
- [x] Every requirement in `docs/superpowers/specs/2026-07-04-model-capability-table-research.md` maps to at least one task above.
- [x] `/v1/models` empty-state behavior is tested and documented.
- [x] Unknown subscription behavior is tested in capability and model-union paths.
- [x] Request routing uses the same capability table as `/v1/models`.
- [x] User affinity cannot force an unsupported credential for a model-specific request.
- [x] Claude Code review gates exist after Task 2, Task 5, and Task 7.
- [x] No implementation step depends on runtime model probing.
- [x] No plan step says to expose Auto, Sonnet 4.0, DeepSeek, MiniMax, GLM, or Qwen before converter support exists.
