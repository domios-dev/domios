//! Claude's one-shot tool approval. Mirrored by `permissionPromptResponse` in
//! `plugins/provider-claude/src/permissionPrompt.ts`; parity-tested.

use codeterm_types::pty::PendingPrompt;

pub(super) const RULE: &str = "claude.tool-permission.yes-once";

/// Tools whose "permission" is a question or plan decision owed to the user.
const USER_DECISION_TOOLS: &[&str] = &["AskUserQuestion", "ExitPlanMode"];

/// Index of the affirmative option, or `None` for anything that is not a
/// single-choice tool approval with an explicit decline path.
pub(super) fn affirmative_option(
    tool_name: &str,
    prompt: &PendingPrompt,
    screen: &str,
) -> Option<usize> {
    let tool = tool_name.trim();
    if tool.is_empty() || USER_DECISION_TOOLS.contains(&tool) {
        return None;
    }
    if prompt.multi_select == Some(true)
        || prompt.submit_available == Some(true)
        || prompt.sections.as_ref().is_some_and(|s| !s.is_empty())
    {
        return None;
    }
    let question = trim_frame(&prompt.question);
    if !question.ends_with('?')
        || question.to_ascii_lowercase().contains("trust")
        || !screen.contains(question)
    {
        return None;
    }
    let labels: Vec<&str> = prompt
        .options
        .iter()
        .map(|o| trim_frame(&o.label))
        .collect();
    if labels.len() < 2 || !labels[0].eq_ignore_ascii_case("yes") {
        return None;
    }
    labels[1..]
        .iter()
        .any(|label| is_decline(label))
        .then_some(0)
}

fn is_decline(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    lower == "no" || lower.starts_with("no,") || lower.starts_with("no ")
}

fn trim_frame(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '│' || c == '┃')
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_types::pty::{PromptOption, PromptSection};

    const SCREEN: &str = "Bash command\n  git status\nDo you want to proceed?\n❯ 1. Yes\n  2. Yes, and don't ask again\n  3. No, and tell Claude what to do differently (esc)\nEsc to cancel";

    fn prompt(question: &str, labels: &[&str]) -> PendingPrompt {
        PendingPrompt {
            question: question.into(),
            options: labels
                .iter()
                .enumerate()
                .map(|(i, label)| PromptOption {
                    label: (*label).into(),
                    value: (i + 1).to_string(),
                })
                .collect(),
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        }
    }

    fn approval() -> PendingPrompt {
        prompt(
            "Do you want to proceed?",
            &[
                "Yes",
                "Yes, and don't ask again",
                "No, and tell Claude what to do differently (esc)",
            ],
        )
    }

    #[test]
    fn tool_approval_selects_the_one_shot_yes() {
        assert_eq!(affirmative_option("Bash", &approval(), SCREEN), Some(0));
    }

    #[test]
    fn user_decision_tools_are_never_answered() {
        assert_eq!(
            affirmative_option("AskUserQuestion", &approval(), SCREEN),
            None
        );
        assert_eq!(
            affirmative_option("ExitPlanMode", &approval(), SCREEN),
            None
        );
        assert_eq!(affirmative_option("  ", &approval(), SCREEN), None);
    }

    #[test]
    fn prompt_not_live_on_screen_is_refused() {
        assert_eq!(affirmative_option("Bash", &approval(), "unrelated"), None);
    }

    #[test]
    fn trust_gates_pickers_and_declineless_prompts_are_refused() {
        let trust = prompt(
            "Is this a project you created or one you trust?",
            &["Yes, I trust this folder", "No, exit"],
        );
        let trust_screen = "Is this a project you created or one you trust?";
        assert_eq!(affirmative_option("Bash", &trust, trust_screen), None);

        let no_decline = prompt("Do you want to proceed?", &["Yes", "Yes, always"]);
        assert_eq!(affirmative_option("Bash", &no_decline, SCREEN), None);

        let first_not_yes = prompt("Do you want to proceed?", &["No", "Yes"]);
        assert_eq!(affirmative_option("Bash", &first_not_yes, SCREEN), None);

        let mut multi = approval();
        multi.multi_select = Some(true);
        assert_eq!(affirmative_option("Bash", &multi, SCREEN), None);

        let mut wizard = approval();
        wizard.sections = Some(vec![PromptSection {
            label: "Step".into(),
            active: true,
            completed: false,
        }]);
        assert_eq!(affirmative_option("Bash", &wizard, SCREEN), None);

        let statement = prompt("Choose an option", &["Yes", "No"]);
        assert_eq!(affirmative_option("Bash", &statement, SCREEN), None);
    }
}
