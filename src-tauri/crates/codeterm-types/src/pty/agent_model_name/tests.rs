use super::*;

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

fn catalogue_ids(provider: &str) -> Vec<String> {
    let path = repo_root()
        .join("plugins")
        .join(format!("provider-{provider}"))
        .join("plugin.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is the catalogue of record: {error}", path.display()));
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("catalogue is JSON");
    let ids: Vec<String> = manifest["models"]
        .as_array()
        .expect("catalogue lists models")
        .iter()
        .filter_map(|model| model["id"].as_str().map(str::to_string))
        .collect();
    assert!(!ids.is_empty(), "{provider} catalogue is empty");
    ids
}

/// The determinism guarantee: every id the shipped catalogues carry resolves to
/// a short name. Adding a catalogue entry whose family is not listed fails here
/// rather than silently rendering that tab as provider-only.
#[test]
fn catalogue_ids_all_resolve() {
    for provider in ["claude", "codex"] {
        for id in catalogue_ids(provider) {
            let short = short_model_name(&id);
            assert!(
                short.is_some(),
                "{provider} catalogue id {id} has no short name; add its family to MODEL_FAMILIES"
            );
            let short = short.unwrap();
            assert!(
                !short.is_empty() && short.len() <= id.len(),
                "{id} must render shorter, got {short}"
            );
        }
    }
}

#[test]
fn a_recognised_codex_model_renders_its_codename() {
    for (id, expected) in [
        ("gpt-5.6-sol", "sol"),
        ("gpt-5.6-luna", "luna"),
        ("gpt-5.6-terra", "terra"),
        ("gpt-6-astra", "astra"),
        ("gpt-5.3-codex-spark", "spark"),
        ("gpt-5.4-mini", "mini"),
    ] {
        assert_eq!(short_model_name(id).as_deref(), Some(expected), "{id}");
    }
}

#[test]
fn an_uncodenamed_catalogue_entry_renders_vendor_and_version() {
    assert_eq!(short_model_name("gpt-5.5").as_deref(), Some("gpt 5.5"));
    assert_eq!(short_model_name("gpt-5").as_deref(), Some("gpt 5"));
    assert_eq!(short_model_name("openai/gpt-5.5").as_deref(), Some("gpt 5.5"));
}

#[test]
fn a_recognised_claude_model_renders_its_family() {
    for (id, expected) in [
        ("opus", "opus"),
        ("Sonnet", "sonnet"),
        ("haiku", "haiku"),
        ("fable", "fable"),
    ] {
        assert_eq!(short_model_name(id).as_deref(), Some(expected), "{id}");
    }
}

/// A vendor id is long and dated; the family inside it is the identity, so a
/// version bump or a dated build must not change what the bracket shows.
#[test]
fn a_long_vendor_id_resolves_to_the_same_family_as_its_short_form() {
    for id in [
        "claude-opus-5",
        "claude-opus-4-5-20250929",
        "anthropic/claude-opus-5",
        "CLAUDE-OPUS-5",
    ] {
        assert_eq!(short_model_name(id).as_deref(), Some("opus"), "{id}");
    }
    assert_eq!(short_model_name("gpt-5.9-sol").as_deref(), Some("sol"));
}

#[test]
fn an_id_no_catalogue_carries_is_unrecognised() {
    for id in [
        "",
        "   ",
        "nvidia/nemotron-3-nano",
        "google/gemma-4-26b-a4b-qat",
        "some-local-gguf",
        "codex",
        "gpt",
    ] {
        assert_eq!(short_model_name(id), None, "{id:?}");
    }
}

/// Resolution reads; it never writes. The caller's stored id is untouched, so
/// falling back to the provider stays a display decision.
#[test]
fn resolving_a_name_leaves_the_stored_id_untouched() {
    let stored = String::from("nvidia/nemotron-3-nano");
    assert_eq!(short_model_name(&stored), None);
    assert_eq!(stored, "nvidia/nemotron-3-nano");
}
