//! Stable Claude aliases stay versionless at execution. Concrete model ids
//! observed from providers normalize back to those aliases for display.

pub fn resolve_model_id(short_or_full: &str) -> &str {
    short_or_full
}

pub fn canonicalize_short(model_id: &str) -> &str {
    if model_id == "opus" || model_id.starts_with("claude-opus-") {
        "opus"
    } else if model_id == "sonnet" || model_id.starts_with("claude-sonnet-") {
        "sonnet"
    } else if model_id == "haiku" || model_id.starts_with("claude-haiku-") {
        "haiku"
    } else if model_id == "fable" || model_id.starts_with("claude-fable-") {
        "fable"
    } else {
        model_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_labels_stay_versionless_at_execution() {
        for label in ["opus", "sonnet", "haiku", "fable"] {
            assert_eq!(resolve_model_id(label), label);
        }
    }

    #[test]
    fn concrete_execution_id_canonicalizes_to_short_label() {
        assert_eq!(canonicalize_short("claude-opus-5"), "opus");
        assert_eq!(canonicalize_short("claude-sonnet-5"), "sonnet");
        assert_eq!(canonicalize_short("claude-haiku-4-5"), "haiku");
        assert_eq!(canonicalize_short("claude-fable-5-1"), "fable");
        assert_eq!(canonicalize_short("sonnet"), "sonnet");
        assert_eq!(canonicalize_short("custom-model-id"), "custom-model-id");
    }

    #[test]
    fn known_short_labels_round_trip_without_a_version_pin() {
        for label in ["opus", "sonnet", "haiku", "fable"] {
            assert_eq!(canonicalize_short(resolve_model_id(label)), label);
        }
    }

    #[test]
    fn concrete_provider_versions_normalize_structurally() {
        for (concrete, family) in [
            ("claude-opus-4-7", "opus"),
            ("claude-opus-5-2", "opus"),
            ("claude-sonnet-4-6", "sonnet"),
            ("claude-haiku-4-5-20251001", "haiku"),
            ("claude-fable-5-1", "fable"),
        ] {
            assert_eq!(canonicalize_short(concrete), family);
        }
        for unrelated in ["anthropic/claude-opus-5", "custom-model-id"] {
            assert_eq!(canonicalize_short(unrelated), unrelated);
        }
    }
}
