//! Stable Claude alias ids → concrete execution ids (JSONL-proven). This
//! mapping owns execution ids only — display and availability live in the
//! provider manifest catalog.

const OPUS_ID: &str = "claude-opus-5";
const SONNET_ID: &str = "claude-sonnet-5";
const HAIKU_ID: &str = "claude-haiku-4-5";
const FABLE_ID: &str = "claude-fable-5";

pub fn resolve_model_id(short_or_full: &str) -> &str {
    match short_or_full {
        "opus" => OPUS_ID,
        "sonnet" => SONNET_ID,
        "haiku" => HAIKU_ID,
        "fable" => FABLE_ID,
        other => other,
    }
}

pub fn canonicalize_short(model_id: &str) -> &str {
    match model_id {
        OPUS_ID | "opus" => "opus",
        SONNET_ID | "sonnet" => "sonnet",
        HAIKU_ID | "haiku" => "haiku",
        FABLE_ID | "fable" => "fable",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_label_resolves_to_concrete_execution_id() {
        assert_eq!(resolve_model_id("opus"), "claude-opus-5");
        assert_eq!(resolve_model_id("sonnet"), "claude-sonnet-5");
        assert_eq!(resolve_model_id("haiku"), "claude-haiku-4-5");
        assert_eq!(resolve_model_id("fable"), "claude-fable-5");
    }

    #[test]
    fn concrete_execution_id_canonicalizes_to_short_label() {
        assert_eq!(canonicalize_short("claude-opus-5"), "opus");
        assert_eq!(canonicalize_short("claude-sonnet-5"), "sonnet");
        assert_eq!(canonicalize_short("claude-haiku-4-5"), "haiku");
        assert_eq!(canonicalize_short("claude-fable-5"), "fable");
        assert_eq!(canonicalize_short("sonnet"), "sonnet");
        assert_eq!(canonicalize_short("custom-model-id"), "custom-model-id");
    }

    #[test]
    fn known_short_labels_resolve_and_round_trip() {
        for label in ["opus", "sonnet", "haiku", "fable"] {
            let concrete = resolve_model_id(label);
            assert_ne!(concrete, label, "short label {label} must resolve");
            assert_eq!(
                canonicalize_short(concrete),
                label,
                "concrete id {concrete} must round-trip to {label}"
            );
        }
    }

    #[test]
    fn no_prior_rollover_pin_remains() {
        for stale in [
            "claude-opus-4-7",
            "claude-opus-4-8",
            "claude-sonnet-4-6",
            "claude-haiku-4-5-20251001",
        ] {
            assert_eq!(
                canonicalize_short(stale),
                stale,
                "stale concrete id {stale} must not be a recognized alias pin"
            );
        }
    }
}
