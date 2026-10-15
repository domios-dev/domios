#[test]
fn update_coordinator_prompt_tab_answers_safe_choice() {
    use codeterm_plugin::provider::AgentProvider;
    use codeterm_types::pty::{PendingPrompt, PromptOption};
    let make_prompt = |options: Vec<(&str, &str)>| PendingPrompt {
        request_id: None,
        question: "opaque".into(),
        options: options
            .into_iter()
            .map(|(label, value)| PromptOption {
                label: label.into(),
                value: value.into(),
            })
            .collect(),
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    let provider = crate::plugin::agent_claude::ClaudeProvider::new();
    let prompt = make_prompt(vec![("Update now", "apply"), ("Skip", "defer")]);
    assert_eq!(
        provider.update_prompt_safe_choice(&prompt).as_deref(),
        Some("defer")
    );
    let missing = make_prompt(vec![("Update now", "apply")]);
    assert_eq!(provider.update_prompt_safe_choice(&missing), None);
}
