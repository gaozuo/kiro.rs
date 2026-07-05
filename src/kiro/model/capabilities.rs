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

    let mut emitted = HashSet::new();
    PAID_MODEL_IDS
        .iter()
        .chain(FREE_MODEL_IDS.iter())
        .copied()
        .filter(|id| selected.contains(id) && emitted.insert(*id))
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
        assert!(!credential_supports_model(
            None,
            "claude-sonnet-4-5-20250929"
        ));
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
            assert!(credential_supports_model(
                Some(title),
                "claude-haiku-4-5-20251001"
            ));
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
