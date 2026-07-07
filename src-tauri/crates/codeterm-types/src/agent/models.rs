//! Single source of truth for Claude model identifiers. One constant changes per Anthropic rollover.

const SONNET_ID: &str = "claude-sonnet-4-6";
const OPUS_ID: &str = "claude-opus-4-7";
const HAIKU_ID: &str = "claude-haiku-4-5-20251001";

pub fn resolve_model_id(short_or_full: &str) -> &str {
    match short_or_full {
        "sonnet" => SONNET_ID,
        "opus" => OPUS_ID,
        "haiku" => HAIKU_ID,
        other => other,
    }
}

pub fn canonicalize_short(model_id: &str) -> &str {
    match model_id {
        SONNET_ID | "sonnet" => "sonnet",
        OPUS_ID | "opus" => "opus",
        HAIKU_ID | "haiku" => "haiku",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_label_resolves_to_concrete_id() {
        assert_eq!(resolve_model_id("sonnet"), "claude-sonnet-4-6");
        assert_eq!(resolve_model_id("opus"), "claude-opus-4-7");
        assert_eq!(resolve_model_id("haiku"), "claude-haiku-4-5-20251001");
    }

    #[test]
    fn legacy_pinned_id_canonicalizes_to_short_label() {
        assert_eq!(canonicalize_short("claude-sonnet-4-6"), "sonnet");
        assert_eq!(canonicalize_short("claude-opus-4-7"), "opus");
        assert_eq!(canonicalize_short("claude-haiku-4-5-20251001"), "haiku");
        assert_eq!(canonicalize_short("sonnet"), "sonnet");
        assert_eq!(canonicalize_short("custom-model-id"), "custom-model-id");
    }

    #[test]
    fn known_short_labels_resolve() {
        for label in ["sonnet", "opus", "haiku"] {
            assert_ne!(
                resolve_model_id(label),
                label,
                "short label {label} must resolve"
            );
        }
    }
}
