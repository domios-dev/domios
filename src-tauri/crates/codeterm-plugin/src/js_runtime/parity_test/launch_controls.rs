use super::*;

#[test]
fn model_launch_args_match_each_cli_flag_format() {
    if require_plugins().is_none() {
        return;
    }
    let cases = [
        ("claude", "--model"),
        ("codex", "-m"),
        ("cursor", "--model"),
        ("opencode", "-m"),
    ];
    for (id, flag) in cases {
        let p = load_js(id);
        assert!(
            p.model_launch_args("").is_empty(),
            "{id}: empty model must produce no launch flags"
        );
        assert_eq!(
            p.model_launch_args("some-model"),
            vec![flag.to_string(), "some-model".to_string()],
            "{id}: model flag format"
        );
    }
}

#[test]
fn supports_stop_matches_stop_sequence() {
    if require_plugins().is_none() {
        return;
    }
    for id in ["claude", "codex", "opencode", "cursor"] {
        let p = load_js(id);
        assert_eq!(
            p.describe().supports_stop,
            p.stop_keys().is_some(),
            "{id}: descriptor supports_stop must equal stop_keys().is_some()"
        );
    }
}

#[test]
fn permission_bypass_descriptor_matches_provider_capability() {
    if require_plugins().is_none() {
        return;
    }
    for id in ["claude", "codex", "opencode", "cursor"] {
        let provider = load_js(id);
        assert_eq!(
            provider.describe().supports_permission_bypass,
            !provider.auto_approve_flags().is_empty(),
            "{id}: UI bypass choice must be derived from provider capability"
        );
    }
}
