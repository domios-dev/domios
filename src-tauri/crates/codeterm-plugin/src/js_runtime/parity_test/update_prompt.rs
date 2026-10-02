use super::*;

#[test]
fn parity_update_prompt_safe_choice() {
    assert!(require_plugins().is_some(), "bundled plugins are required");
    let fixtures = [
        (vec![("apply", "Update now"), ("skip", "Skip")], Some("skip")),
        (vec![("1", "Update now"), ("2", "Later")], Some("2")),
        (vec![("apply", "Update now"), ("postpone", "Postpone")], Some("postpone")),
        (vec![("1", "Yes"), ("2", "No")], None),
        (vec![("1", "Update now"), ("2", "Cancel")], None),
        (vec![("1", "Update now"), ("2", "Skip"), ("3", "Later")], None),
        (vec![("1", "Update now"), ("1", "Skip")], None),
        (vec![("1", "Update later"), ("2", "Update now")], None),
    ];
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    for (options, expected) in fixtures {
        let prompt = crate::prompt::build_prompt("fixture-question".into(), options.into_iter().map(|(v, l)| (v.into(), l.into())).collect());
        assert_eq!(native.update_prompt_safe_choice(&prompt).as_deref(), expected);
        for id in ["claude", "codex", "opencode"] {
            assert_eq!(load_js(id).update_prompt_safe_choice(&prompt), native.update_prompt_safe_choice(&prompt), "{id}");
        }
    }
}
