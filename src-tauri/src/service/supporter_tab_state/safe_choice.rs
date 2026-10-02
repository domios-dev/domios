use super::TabStateMetadata;
use codeterm_types::decision::DecisionQuestion;
use std::collections::BTreeMap;

pub(crate) fn safe_choice_question(metadata: &TabStateMetadata) -> Option<DecisionQuestion> {
    let declared = metadata.declared_safe_choice.as_ref()?;
    if metadata.prompt_options.len() < 2
        || metadata
            .prompt_options
            .iter()
            .filter(|option| &option.value == declared)
            .count()
            != 1
    {
        return None;
    }
    if metadata.context.get("pending_prompt_kind")?.as_str()? != "manual_update" {
        return None;
    }
    let mut options: BTreeMap<_, _> = metadata
        .prompt_options
        .iter()
        .map(|option| (option.value.clone(), option.label.clone()))
        .collect();
    if options.len() != metadata.prompt_options.len() || options.contains_key("__abstain__") {
        return None;
    }
    options.insert(
        "__abstain__".into(),
        "No safe update choice is visible".into(),
    );
    Some(DecisionQuestion::Choice { options })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::supporter_tab_state::NumberedPromptOption;

    fn metadata() -> TabStateMetadata {
        TabStateMetadata {
            context: serde_json::json!({"pending_prompt_kind": "manual_update"}),
            session_key: "fixture".into(),
            title: "fixture".into(),
            spawning_tab_id: None,
            parent_tab_id: None,
            plan_id: None,
            task_id: None,
            attempt_id: None,
            prompt_options: vec![
                NumberedPromptOption {
                    number: 1,
                    label: "opaque-a".into(),
                    value: "a".into(),
                },
                NumberedPromptOption {
                    number: 2,
                    label: "opaque-b".into(),
                    value: "b".into(),
                },
            ],
            declared_safe_choice: Some("b".into()),
            tab_snapshot: None,
            process_tree_key: None,
        }
    }

    #[test]
    fn ga_safe_choice_question_contains_only_declared_values_and_abstention() {
        let DecisionQuestion::Choice { options } = safe_choice_question(&metadata()).unwrap()
        else {
            panic!("choice question");
        };
        assert_eq!(
            options.keys().map(String::as_str).collect::<Vec<_>>(),
            ["__abstain__", "a", "b"]
        );
        let mut missing = metadata();
        missing.declared_safe_choice = None;
        assert!(safe_choice_question(&missing).is_none());
        missing = metadata();
        missing.prompt_options[1].value = "a".into();
        assert!(safe_choice_question(&missing).is_none());
        missing = metadata();
        missing.context["pending_prompt_kind"] = serde_json::json!("provider");
        assert!(safe_choice_question(&missing).is_none());
    }
}
