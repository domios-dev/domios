pub(super) fn has_current_prompt(parsed_prompt: bool, command_owned: bool) -> bool {
    parsed_prompt && !command_owned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_event_without_a_current_provider_prompt_cannot_set_waiting() {
        assert!(!has_current_prompt(false, false));
        assert!(!has_current_prompt(false, true));
        assert!(!has_current_prompt(true, true));
        assert!(has_current_prompt(true, false));
    }
}
