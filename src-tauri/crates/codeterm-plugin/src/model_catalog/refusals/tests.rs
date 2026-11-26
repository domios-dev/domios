use super::*;

fn scope(account: &str, version: &str) -> Scope {
    Scope {
        account: account.into(),
        cli_version: version.into(),
        credentials: Vec::new(),
        cli_binary: "fixture".into(),
        cli_install_dirs: Vec::new(),
        binary: BinaryFingerprint {
            path: "fixture".into(),
            len: 1,
            modified_unix: 1,
        },
    }
}

#[test]
fn catalogue_model_refusal_tracks_the_exact_model_account_and_cli_version() {
    let mut state = Refusals::default();
    assert!(state.remember(scope("first", "1.0.0"), "model-a"));
    assert!(!state.remember(scope("first", "1.0.0"), "model-a"));
    assert!(state.remember(scope("first", "1.0.0"), "model-b"));
    assert_eq!(state.models, ["model-a", "model-b"]);
    assert!(state.remember(scope("second", "1.0.0"), "model-b"));
    assert_eq!(state.models, ["model-b"]);
    assert!(state.remember(scope("second", "1.0.1"), "model-a"));
    assert_eq!(state.models, ["model-a"]);
    assert_eq!(state.scope, Some(scope("second", "1.0.1")));
}
