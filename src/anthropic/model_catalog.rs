use std::collections::HashSet;

use super::types::Model;

pub fn supported_models() -> Vec<Model> {
    vec![
        Model {
            id: "claude-sonnet-5".to_string(),
            object: "model".to_string(),
            created: 1782777600,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Sonnet 5".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 128_000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(128_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-sonnet-5-thinking".to_string(),
            object: "model".to_string(),
            created: 1782777600,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Sonnet 5 (Thinking)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 128_000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(128_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-sonnet-5-agentic".to_string(),
            object: "model".to_string(),
            created: 1782777600,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Sonnet 5 (Agentic)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 128_000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(128_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-sonnet-4-6".to_string(),
            object: "model".to_string(),
            created: 1770314400,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Sonnet 4.6".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(64_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-sonnet-4-6-thinking".to_string(),
            object: "model".to_string(),
            created: 1770314400,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Sonnet 4.6 (Thinking)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(64_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-sonnet-4-6-agentic".to_string(),
            object: "model".to_string(),
            created: 1770314400,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Sonnet 4.6 (Agentic)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(64_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-sonnet-4-5-20250929".to_string(),
            object: "model".to_string(),
            created: 1727568000,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Sonnet 4.5".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(200_000),
            max_completion_tokens: Some(64_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-sonnet-4-5-20250929-thinking".to_string(),
            object: "model".to_string(),
            created: 1727568000,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Sonnet 4.5 (Thinking)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(200_000),
            max_completion_tokens: Some(64_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-sonnet-4-5-20250929-agentic".to_string(),
            object: "model".to_string(),
            created: 1727568000,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Sonnet 4.5 (Agentic)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(200_000),
            max_completion_tokens: Some(64_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-opus-4-5-20251101".to_string(),
            object: "model".to_string(),
            created: 1730419200,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Opus 4.5".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(200_000),
            max_completion_tokens: Some(64_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-opus-4-5-20251101-thinking".to_string(),
            object: "model".to_string(),
            created: 1730419200,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Opus 4.5 (Thinking)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(200_000),
            max_completion_tokens: Some(64_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-opus-4-5-20251101-agentic".to_string(),
            object: "model".to_string(),
            created: 1730419200,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Opus 4.5 (Agentic)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(200_000),
            max_completion_tokens: Some(64_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-opus-4-6".to_string(),
            object: "model".to_string(),
            created: 1770314400,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Opus 4.6".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(128_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-opus-4-6-thinking".to_string(),
            object: "model".to_string(),
            created: 1770314400,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Opus 4.6 (Thinking)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(128_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-opus-4-6-agentic".to_string(),
            object: "model".to_string(),
            created: 1770314400,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Opus 4.6 (Agentic)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(128_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-opus-4-7".to_string(),
            object: "model".to_string(),
            created: 1772992800,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Opus 4.7".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(128_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-opus-4-7-thinking".to_string(),
            object: "model".to_string(),
            created: 1772992800,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Opus 4.7 (Thinking)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(128_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-opus-4-7-agentic".to_string(),
            object: "model".to_string(),
            created: 1772992800,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Opus 4.7 (Agentic)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(128_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-opus-4-8".to_string(),
            object: "model".to_string(),
            created: 1775671200,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Opus 4.8".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(128_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-opus-4-8-thinking".to_string(),
            object: "model".to_string(),
            created: 1775671200,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Opus 4.8 (Thinking)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(128_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-opus-4-8-agentic".to_string(),
            object: "model".to_string(),
            created: 1775671200,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Opus 4.8 (Agentic)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(1_000_000),
            max_completion_tokens: Some(128_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-haiku-4-5-20251001".to_string(),
            object: "model".to_string(),
            created: 1727740800,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Haiku 4.5".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(200_000),
            max_completion_tokens: Some(64_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-haiku-4-5-20251001-thinking".to_string(),
            object: "model".to_string(),
            created: 1727740800,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Haiku 4.5 (Thinking)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(200_000),
            max_completion_tokens: Some(64_000),
            thinking: Some(true),
        },
        Model {
            id: "claude-haiku-4-5-20251001-agentic".to_string(),
            object: "model".to_string(),
            created: 1727740800,
            owned_by: "anthropic".to_string(),
            display_name: "Claude Haiku 4.5 (Agentic)".to_string(),
            model_type: "chat".to_string(),
            max_tokens: 32000,
            context_length: Some(200_000),
            max_completion_tokens: Some(64_000),
            thinking: Some(true),
        },
    ]
}

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
