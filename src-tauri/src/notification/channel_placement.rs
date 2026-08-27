//! Where a channel put the message that carried a question, and what that
//! message becomes once the question is spent.
//!
//! A locator is written by the channel that sent the message and read back by
//! that same channel. Nothing here parses one: core keeps the pair and hands it
//! back, so a channel this build has never met stores its address the same way.

use serde::Serialize;

use super::channel_ask::PushMessage;
use super::push;
use super::store::{Notification, NotificationProvenance};

/// One channel's own address for a message it placed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChannelPlacement {
    pub service: String,
    pub locator: String,
}

/// Remember where a channel put this record's message. A fan to several
/// recipients is several placements; the same message reported twice is one.
pub fn remember(
    notifications: &mut [Notification],
    id: &str,
    service: &str,
    locator: &str,
) -> bool {
    let Some(target) = notifications.iter_mut().find(|n| n.id == id) else {
        return false;
    };
    let placement = ChannelPlacement {
        service: service.trim().to_string(),
        locator: locator.trim().to_string(),
    };
    if placement.service.is_empty() || placement.locator.is_empty() {
        return false;
    }
    if target.placements.contains(&placement) {
        return true;
    }
    target.placements.push(placement);
    true
}

pub fn placements(notifications: &[Notification], id: &str) -> Vec<ChannelPlacement> {
    notifications
        .iter()
        .find(|n| n.id == id)
        .map(|n| n.placements.clone())
        .unwrap_or_default()
}

/// One spent question, and every channel message that still presents it as
/// open. The rewrite carries no choices: that is what makes it spent.
///
/// Held unrendered for the same reason a push is: the length bound belongs to
/// the channel doing the rewrite, not to whoever answered.
#[derive(Debug, Clone)]
pub struct SpentQuestion {
    pub placements: Vec<ChannelPlacement>,
    content: push::PushContent,
    label: String,
    record: String,
}

impl SpentQuestion {
    /// The rewrite one channel receives, bounded by that channel. The answer
    /// line is reserved out of the budget so appending it cannot overrun.
    pub fn message_for(&self, bounds: push::PushBounds) -> PushMessage {
        let reserved = spent_text("", &self.label).chars().count();
        PushMessage {
            text: spent_text(
                &self.content.render(bounds.reserving(reserved)),
                &self.label,
            ),
            choices: Vec::new(),
            record: self.record.clone(),
        }
    }
}

/// What one record owes the channels that carried it, now that it holds an
/// answer. `None` when nothing was placed or no answer stands, so a question
/// still open is never rewritten.
pub fn spent_question(notifications: &[Notification], id: &str) -> Option<SpentQuestion> {
    let target = notifications.iter().find(|n| n.id == id)?;
    if target.placements.is_empty() {
        return None;
    }
    let label = target.answer.as_ref()?.answered.as_deref()?;
    Some(SpentQuestion {
        placements: target.placements.clone(),
        content: push::PushContent {
            level: target.level?,
            title: target.title.clone(),
            body: target.body.clone(),
            provenance: NotificationProvenance {
                tab_id: target.tab_id.clone(),
                tab_title: target.tab_title.clone(),
                workspace_id: target.workspace_id.clone(),
                peer_name: target.peer_name.clone(),
            },
        },
        label: label.to_string(),
        record: target.id.clone(),
    })
}

/// What a spent question leaves in the chat: the question as it was carried,
/// and the decision it now records. Same meaning the in-app card carries, so
/// the two surfaces do not describe one answer in two vocabularies.
pub fn spent_text(carried: &str, label: &str) -> String {
    format!("{}\n\nAnswered: {}", carried.trim_end(), label.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::push::PushBounds;
    use crate::notification::store::{NotificationSeverity, NotificationStore};
    use codeterm_types::notification::{adopt_caller_actions, answer_actions};

    const BOUNDS: PushBounds = PushBounds::for_ceiling(4096);

    fn one_record() -> (NotificationStore, String) {
        let mut store = NotificationStore::new(10);
        let id = store.add_ext(
            "Deploy?".into(),
            None,
            NotificationSeverity::Info,
            NotificationProvenance::default(),
            crate::notification::store::Ask::default(),
            None,
            None,
        );
        (store, id)
    }

    #[test]
    fn spent_question_placements_accumulate_per_recipient_and_never_duplicate() {
        let (mut store, id) = one_record();
        assert!(store.remember_channel_placement(&id, "telegram", "5:9"));
        assert!(store.remember_channel_placement(&id, "telegram", "6:9"));
        assert!(store.remember_channel_placement(&id, "telegram", "5:9"));
        assert_eq!(store.channel_placements(&id).len(), 2);
    }

    #[test]
    fn spent_question_a_record_nobody_stored_holds_no_placement() {
        let (mut store, _) = one_record();
        assert!(!store.remember_channel_placement("no-such-record", "telegram", "5:9"));
        assert!(store.channel_placements("no-such-record").is_empty());
    }

    #[test]
    fn spent_question_is_owed_only_once_an_answer_stands() {
        let mut store = NotificationStore::new(10);
        let id = store.add_ext(
            "Deploy?".into(),
            None,
            NotificationSeverity::Info,
            NotificationProvenance {
                tab_id: Some("tab-asking".into()),
                ..Default::default()
            },
            crate::notification::store::Ask::offering(Some(adopt_caller_actions(
                answer_actions(&["Ship it".to_string(), "Hold".to_string()]).expect("legal set"),
            ))),
            None,
            Some(crate::notification::push::NotifyLevel::ActionNeeded),
        );
        store.remember_channel_placement(&id, "telegram", "5:9");
        assert!(
            store.spent_question(&id).is_none(),
            "an open question is not spent"
        );

        store.record_answer(&id, "Ship it");
        let spent = store
            .spent_question(&id)
            .expect("an answered question is spent");
        let rewrite = spent.message_for(BOUNDS);
        assert!(rewrite.text.contains("Ship it"));
        assert!(!rewrite.offers_choices());
        assert_eq!(spent.placements.len(), 1);
    }

    #[test]
    fn spent_question_text_names_the_chosen_label() {
        let text = spent_text("[action needed] Deploy?", "Ship it");
        assert!(text.contains("Ship it"));
        assert!(text.contains("Deploy?"));
    }
}
