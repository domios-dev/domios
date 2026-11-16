use super::*;
use crate::provider::AgentProvider;

#[test]
fn external_imports_native_js_shared_fixtures() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let dir = workspace_plugins_root().join("provider-claude/tests/fixtures/external-imports");
    let cases: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("cases.json")).unwrap()).unwrap();
    for case in cases.as_array().unwrap() {
        let file = case["file"].as_str().unwrap();
        let screen = std::fs::read_to_string(dir.join(file)).unwrap();
        let active = case["active"].as_bool().unwrap();
        let prompt = native.parse_prompt(&screen);
        assert_eq!(prompt.is_some(), active, "{file}");
        assert_eq!(prompt, js.parse_prompt(&screen), "{file}");
        for provider in [&native as &dyn AgentProvider, &js as &dyn AgentProvider] {
            assert!(
                matches!(
                    provider.directory_trust_prompt_response(&screen),
                    None | Some(crate::provider::DirectoryTrustPromptResponse::Hold)
                ),
                "{file}"
            );
            assert!(
                provider.parse_directory_trust_prompt(&screen).is_none(),
                "{file}"
            );
            assert!(
                provider.launch_onboarding_response(&screen).is_none(),
                "{file}"
            );
            assert!(
                provider.launch_onboarding_safe_choice(&screen).is_none(),
                "{file}"
            );
        }
        if let Some(prompt) = prompt {
            assert_eq!(
                prompt.question,
                "Claude is asking whether to allow external CLAUDE.md imports"
            );
            assert_eq!(prompt.options[0].label, "No, disable external imports");
            assert_eq!(prompt.options[1].label, "Yes, allow external imports");
            let snapshot = crate::tab_state::TabStateSnapshot {
                tab_id: "fixture".into(),
                agent_type: "fixture".into(),
                session_id: None,
                cwd: None,
                screen_text: screen.clone(),
            };
            let verdict = native.classify_tab_state(&snapshot).unwrap();
            assert_eq!(
                Some(verdict.clone()),
                js.classify_tab_state(&snapshot),
                "{file}"
            );
            assert_eq!(
                verdict.state,
                crate::tab_state::TabState::ClarifyingQuestion
            );
            let action = verdict.action.unwrap();
            assert!(action.keys.is_none());
            assert!(action.delayed_resume.is_none());
            let focus = case["focus"].as_u64().unwrap() as usize;
            for target in 0..2 {
                let expected = if target == focus {
                    vec![b"\r".to_vec()]
                } else {
                    vec![
                        if target > focus {
                            b"j".to_vec()
                        } else {
                            b"k".to_vec()
                        },
                        b"\r".to_vec(),
                    ]
                };
                assert_eq!(
                    native.build_prompt_response(target, &screen),
                    expected,
                    "{file}"
                );
                assert_eq!(
                    js.build_prompt_response(target, &screen),
                    expected,
                    "{file}"
                );
            }
            for provider in [&native as &dyn AgentProvider, &js as &dyn AgentProvider] {
                assert!(provider.update_prompt_safe_choice(&prompt).is_none());
                assert!(provider
                    .permission_prompt_response("Bash", &prompt, &screen)
                    .is_none());
            }
        }
    }
}
