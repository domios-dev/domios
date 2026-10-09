use crate::provider::{
    ModelSwitchCommand, ModelSwitchCommands, ModelSwitchPrompt, ModelSwitchTranscript,
};

pub(super) fn commands() -> ModelSwitchCommands {
    ModelSwitchCommands {
        transcript: ModelSwitchTranscript {
            record_type: "user".into(),
            content_path: vec!["message".into(), "content".into()],
            prefix: "<local-command-stdout>".into(),
            suffix: "</local-command-stdout>".into(),
        },
        model: ModelSwitchCommand {
            command: "/model {model}".into(),
            confirmation: "(?i)^Set model to .*?(?:{model}|{display_name})(?:[^a-z0-9_-]|$)".into(),
            prompt: Some(ModelSwitchPrompt {
                screen_regex: "(?im)^\\s*Switch model\\?\\s*$[\\s\\S]*^\\s*❯ 1\\. Yes, switch to (?:{model}|{display_name})(?: [^\\r\\n]+)?\\s*$[\\s\\S]*^\\s*2\\. No, go back\\s*\\z".into(),
                response_keys: vec!["\r".into()],
                cancel_keys: vec!["\u{1b}".into()],
            }),
        },
        effort: Some(ModelSwitchCommand {
            command: "/effort {effort}".into(),
            confirmation: "(?i)^Set effort level to {effort}(?:[^a-z0-9_-]|$)".into(),
            prompt: None,
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
