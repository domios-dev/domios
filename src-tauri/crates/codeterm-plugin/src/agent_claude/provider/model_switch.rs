use crate::provider::ModelSwitchCommands;

pub(super) fn commands() -> ModelSwitchCommands {
    super::schemas::model_switch_commands()
}

pub(super) fn confirmation_prompt(
    screen: &str,
    selection: &str,
) -> Option<codeterm_types::pty::PendingPrompt> {
    if selection.is_empty()
        || selection
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return None;
    }
    let lines = screen.trim_end().lines().collect::<Vec<_>>();
    let row = regex::Regex::new(r"^\s*(❯\s*)?([12])\.\s+(.+?)\s*$").ok()?;
    let first = row.captures(lines.get(lines.len().checked_sub(2)?)?)?;
    let second = row.captures(lines.last()?)?;
    if &first[2] != "1" || &second[2] != "2" || first.get(1).is_none() || second.get(1).is_some() {
        return None;
    }
    let before = &lines[..lines.len() - 2];
    let question = before.iter().rposition(|line| line.trim().ends_with('?'))?;
    if !before[question + 1..]
        .iter()
        .any(|line| !line.trim().is_empty())
        || before[question..]
            .iter()
            .any(|line| line.contains(['│', '┃', '┌', '└', '☐', '☑']) || row.is_match(line))
    {
        return None;
    }
    let token = regex::RegexBuilder::new(&format!(
        r"(?:^|[^a-z0-9_-]){}(?:$|[^a-z0-9_-])",
        regex::escape(selection)
    ))
    .case_insensitive(true)
    .build()
    .ok()?;
    if !token.is_match(&first[3]) || token.is_match(&second[3]) {
        return None;
    }
    Some(crate::prompt::build_prompt(
        before[question].trim().to_owned(),
        vec![
            (first[3].to_owned(), "1".into()),
            (second[3].to_owned(), "2".into()),
        ],
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn declared_model_confirmation_matches_captured_screen_and_focus() {
        let screen = include_str!("../../../../../../plugins/provider-claude/tests/fixtures/model-switch/sonnet-confirmation.txt");
        assert!(super::confirmation_prompt(screen, "sonnet").is_some());
        assert!(super::confirmation_prompt(screen, "opus").is_none());
        assert!(super::confirmation_prompt(&screen.replace("❯ 1.", "  1."), "sonnet").is_none());
        assert!(
            super::confirmation_prompt(&format!("{screen}\nUnrelated prompt"), "sonnet").is_none()
        );
        assert!(super::commands().model.confirmation_prompt_kind.is_some());
        assert!(super::commands()
            .effort
            .unwrap()
            .confirmation_prompt_kind
            .is_some());
    }
}
