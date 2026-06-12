//! General Agent blocker auto-acknowledge detection.

use crate::multiplexer::types::PendingPrompt;

#[derive(Debug, Clone, PartialEq)]
pub struct AutoAck {
    pub option_index: usize,
    pub kind: AutoAckKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AutoAckKind {
    FirstAffirmative,
    RepairConfiguration,
}

impl AutoAckKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::FirstAffirmative => "affirmative blocker",
            Self::RepairConfiguration => "configuration repair menu",
        }
    }
}

pub fn classify(prompt: &PendingPrompt, screen: &str) -> Option<AutoAck> {
    classify_repair_menu(prompt, screen).or_else(|| classify_first_affirmative(prompt))
}

fn classify_first_affirmative(prompt: &PendingPrompt) -> Option<AutoAck> {
    let first = prompt.options.first()?;
    let label = first.label.to_ascii_lowercase();
    let value = first.value.to_ascii_lowercase();
    if value == "y" || value == "yes" || label.starts_with("yes") {
        Some(AutoAck {
            option_index: 0,
            kind: AutoAckKind::FirstAffirmative,
        })
    } else {
        None
    }
}

fn classify_repair_menu(prompt: &PendingPrompt, screen: &str) -> Option<AutoAck> {
    if prompt.options.len() < 2 {
        return None;
    }
    if !crate::agent::detector::detect_interactive_prompt(screen) {
        return None;
    }

    let mut score = 0u8;
    score += 2; // Active numbered picker structure was already parsed.

    let lower_screen = screen.to_ascii_lowercase();
    if has_any(&lower_screen, &["error", "failed", "invalid", "corrupt"]) {
        score += 1;
    }
    if has_any(&lower_screen, &["config", "settings", "configuration"]) {
        score += 1;
    }
    if has_any(&lower_screen, &["json", "parse", "syntax"]) {
        score += 1;
    }

    let repair_idx = prompt.options.iter().position(|option| {
        let label = option.label.to_ascii_lowercase();
        has_any(&label, &["reset", "repair", "restore", "recreate"])
    });
    if repair_idx.is_some() {
        score += 3;
    }

    if prompt.options.iter().any(|option| {
        let label = option.label.to_ascii_lowercase();
        has_any(&label, &["exit", "quit", "cancel"])
    }) {
        score += 1;
    }

    if score >= 7 {
        repair_idx.map(|option_index| AutoAck {
            option_index,
            kind: AutoAckKind::RepairConfiguration,
        })
    } else {
        None
    }
}

fn has_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::detector::parse_permission_prompt;
    use crate::multiplexer::types::{PendingPrompt, PromptOption};

    fn prompt_from_screen(screen: &str) -> PendingPrompt {
        let (question, options) = parse_permission_prompt(screen).expect("prompt parsed");
        PendingPrompt {
            question,
            options: options
                .into_iter()
                .map(|(label, value)| PromptOption { label, value })
                .collect(),
        }
    }

    #[test]
    fn claude_invalid_json_config_menu_selects_reset() {
        let screen = "\
Configuration error

Could not read settings file: invalid JSON near line 1.

Choose an option:
  ❯ 1. Exit and fix manually
    2. Reset with default configuration

Enter to confirm · Esc to cancel";
        let prompt = prompt_from_screen(screen);
        let ack = classify(&prompt, screen).expect("classified");
        assert_eq!(ack.kind, AutoAckKind::RepairConfiguration);
        assert_eq!(ack.option_index, 1);
    }

    #[test]
    fn config_repair_requires_screen_error_context() {
        let screen = "\
Choose an option:
  ❯ 1. Exit
    2. Reset

Enter to confirm · Esc to cancel";
        let prompt = prompt_from_screen(screen);
        assert_eq!(classify(&prompt, screen), None);
    }

    #[test]
    fn trust_prompt_selects_first_affirmative() {
        let screen = "\
? Do you trust the contents of this directory?

  ❯ 1. Yes, continue
    2. No, quit

Enter to confirm · Esc to cancel";
        let prompt = prompt_from_screen(screen);
        let ack = classify(&prompt, screen).expect("classified");
        assert_eq!(ack.kind, AutoAckKind::FirstAffirmative);
        assert_eq!(ack.option_index, 0);
    }

    #[test]
    fn plain_numbered_answer_is_not_a_blocker() {
        let prompt = PendingPrompt {
            question: "Plan".into(),
            options: vec![
                PromptOption {
                    label: "Reset parser state".into(),
                    value: "1".into(),
                },
                PromptOption {
                    label: "Exit loop".into(),
                    value: "2".into(),
                },
            ],
        };
        assert_eq!(
            classify(
                &prompt,
                "Here is a plan:\n1. Reset parser state\n2. Exit loop"
            ),
            None
        );
    }
}
