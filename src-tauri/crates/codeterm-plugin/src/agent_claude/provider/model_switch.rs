use crate::provider::{
    ModelSwitchCommand, ModelSwitchCommands, ModelSwitchPicker, ModelSwitchPickerPosition,
    ModelSwitchPrompt, ModelSwitchTranscript,
};

pub(super) fn commands() -> ModelSwitchCommands {
    ModelSwitchCommands {
        confirmation_source: Default::default(),
        transcript: ModelSwitchTranscript {
            record_type: "user".into(),
            content_path: vec!["message".into(), "content".into()],
            prefix: "<local-command-stdout>".into(),
            suffix: "</local-command-stdout>".into(),
        },
        model: ModelSwitchCommand {
            command: "/model".into(),
            confirmation: r#"(?i)^Set model to `?(?:{model}|{display_name})(?: [^`\r\n]+)?`? for this session only(?:$|[^a-z0-9_-])"#.into(),
            prompt: Some(ModelSwitchPrompt {
                screen_regex: r#"(?im)^\s*Switch model\?\s*$[\s\S]*^\s*❯\s*1\. Yes, switch to (?:{model}|{display_name})(?: [^\r\n]+)?\s*$[\s\S]*^\s*2\. No, go back\s*(?:\r?\n\s*(?:Enter to confirm|Esc to cancel)[^\r\n]*)?\s*\z"#.into(),
                response_keys: vec!["\r".into()],
                cancel_keys: vec!["\u{1b}".into()],
            }),
            picker: Some(ModelSwitchPicker {
                screen_regex: r#"(?m)^\s*Select model\s*$[\s\S]*^\s*Enter to set as default · s to use this session only · Esc to cancel\s*\z"#.into(),
                selected_regex: r#"(?m)^\s*❯\s*(?P<position>\d+)\.\s+[^\r\n]+$"#.into(),
                target_regex: r#"(?im)^\s*(?:❯\s*)?(?P<position>\d+)\.\s+(?:{model}|{display_name})(?:\s+[^\r\n]+)?$"#.into(),
                position: ModelSwitchPickerPosition::Number,
                previous_key: "\u{1b}[A".into(),
                next_key: "\u{1b}[B".into(),
                response_key: "s".into(),
                cancel_key: "\u{1b}".into(),
            }),
        },
        effort: Some(ModelSwitchCommand {
            command: "/effort".into(),
            confirmation: r#"(?i)^Set effort level to {effort} \(this session only\)(?:$|:)"#.into(),
            prompt: Some(ModelSwitchPrompt {
                screen_regex: r#"(?im)^\s*Change effort level\?\s*$[\s\S]*^\s*❯\s*1\. Yes, switch to {effort}\s*$[\s\S]*^\s*2\. No, go back\s*(?:\r?\n\s*(?:Enter to confirm|Esc to cancel)[^\r\n]*)?\s*\z"#.into(),
                response_keys: vec!["\r".into()],
                cancel_keys: vec!["\u{1b}".into()],
            }),
            picker: Some(ModelSwitchPicker {
                screen_regex: r#"(?m)^\s*Effort\s*$[\s\S]*^\s*←/→ to adjust · Enter to confirm · s for this session only · Esc to cancel\s*\z"#.into(),
                selected_regex: r#"(?m)^(?P<position>[^\r\n▲]*)▲[^\r\n]*$"#.into(),
                target_regex: r#"(?im)^(?P<position>[^\r\n]*?)(?P<label>\b{effort}\b)[^\r\n]*\s+Tab to toggle\s*$"#.into(),
                position: ModelSwitchPickerPosition::Column,
                previous_key: "\u{1b}[D".into(),
                next_key: "\u{1b}[C".into(),
                response_key: "s".into(),
                cancel_key: "\u{1b}".into(),
            }),
        }),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn declared_model_confirmation_matches_captured_screen_and_focus() {
        let screen = include_str!("../../../../../../plugins/provider-claude/tests/fixtures/model-switch/sonnet-confirmation.txt");
        let declaration = super::commands().model.prompt.unwrap();
        let pattern = regex::Regex::new(
            &declaration
                .screen_regex
                .replace("{model}", "sonnet")
                .replace("{display_name}", "Claude Sonnet"),
        )
        .unwrap();
        assert!(pattern.is_match(screen));
        assert!(!pattern.is_match(&screen.replace("❯ 1.", "  1.").replace("  2.", "❯ 2.")));
        assert!(!pattern.is_match(&screen.replace("Sonnet", "Opus")));
        assert!(!pattern.is_match(&format!("{screen}\nUnrelated prompt")));
        assert_eq!(declaration.response_keys, vec!["\r"]);
        assert_eq!(declaration.cancel_keys, vec!["\u{1b}"]);
    }
}
