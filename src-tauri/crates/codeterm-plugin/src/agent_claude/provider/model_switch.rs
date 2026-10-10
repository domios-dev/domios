use crate::provider::ModelSwitchCommands;

pub(super) fn commands() -> ModelSwitchCommands {
    super::schemas::model_switch_commands()
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
