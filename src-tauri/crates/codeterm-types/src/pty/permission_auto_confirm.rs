use serde::{Deserialize, Serialize};

/// Most recent auto-confirmations kept on a tab for the Chat UI; the durable
/// audit lives in the tab lifecycle journal.
pub const MAX_RECENT_AUTO_CONFIRMATIONS: usize = 20;

/// One provider permission prompt answered by a tab's opt-in auto-confirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoConfirmedPrompt {
    pub tool_name: String,
    pub option_label: String,
    pub rule: String,
    pub question: String,
    pub options: Vec<String>,
    pub prompt_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub confirmed_at_ms: u64,
}

pub fn record_auto_confirmation(recent: &mut Vec<AutoConfirmedPrompt>, entry: AutoConfirmedPrompt) {
    recent.push(entry);
    let overflow = recent.len().saturating_sub(MAX_RECENT_AUTO_CONFIRMATIONS);
    if overflow > 0 {
        recent.drain(0..overflow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(n: u64) -> AutoConfirmedPrompt {
        AutoConfirmedPrompt {
            tool_name: format!("tool-{n}"),
            option_label: "Yes".into(),
            rule: "rule".into(),
            question: "q".into(),
            options: vec!["Yes".into(), "No".into()],
            prompt_fingerprint: format!("fp-{n}"),
            session_id: None,
            confirmed_at_ms: n,
        }
    }

    #[test]
    fn recent_auto_confirmations_stay_bounded_and_keep_the_newest() {
        let mut recent = Vec::new();
        for n in 0..(MAX_RECENT_AUTO_CONFIRMATIONS as u64 + 5) {
            record_auto_confirmation(&mut recent, entry(n));
        }
        assert_eq!(recent.len(), MAX_RECENT_AUTO_CONFIRMATIONS);
        assert_eq!(recent.first().unwrap().confirmed_at_ms, 5);
        assert_eq!(
            recent.last().unwrap().confirmed_at_ms,
            MAX_RECENT_AUTO_CONFIRMATIONS as u64 + 4
        );
    }
}
