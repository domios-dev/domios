use serde::Serialize;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use codeterm_types::notification::{AnswerSlot, NotificationAction};

use super::answer::{self, AnswerAdmission};
use super::answer_delivery::{self, AnswerDeliveryPlan};
use super::dedupe::{
    ConditionLedger, ConditionTransition, DeliveryAdmission, DeliveryLedger, NotificationIdentity,
};
use super::push::NotifyLevel;

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub id: String,
    pub tab_id: Option<String>,
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_name: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub severity: NotificationSeverity,
    /// The axis a caller may name explicitly, independent of `severity`. Absent
    /// is a legal, distinct state, never defaulted from `severity` here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<NotifyLevel>,
    pub timestamp: u64,
    pub read: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<NotificationAction>>,
    /// Opened when the record offers a choice. Carries the asking tab captured
    /// at send time and the one answer it may ever hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<AnswerSlot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticky: Option<bool>,
    /// Set when the push sink refused the record. Never surfaced to the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push_failed: Option<bool>,
}

/// Daemon-owned origin of a notification. Every field is resolved against live
/// daemon state; nothing here may be stored straight from a caller payload.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NotificationProvenance {
    pub tab_id: Option<String>,
    pub tab_title: Option<String>,
    pub workspace_id: Option<String>,
    pub peer_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub enum NotificationSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DedupAdmission {
    Added(String),
    Duplicate(String),
}

/// Outcome of push admission. Only `Deliver` and `Supersede` may reach a sink;
/// every variant still leaves the caller with a durable record id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushAdmission {
    Deliver(String),
    /// Same event key while the record is still open: no second record, no push.
    Duplicate(String),
    /// Novel event key: the open record is rewritten in place and pushed once.
    Supersede(String),
    /// Lifetime ceiling reached: the record still lands, the push does not.
    Suppressed(String),
}

impl PushAdmission {
    pub fn id(&self) -> &str {
        match self {
            Self::Deliver(id)
            | Self::Duplicate(id)
            | Self::Supersede(id)
            | Self::Suppressed(id) => id,
        }
    }

    pub fn should_push(&self) -> bool {
        matches!(self, Self::Deliver(_) | Self::Supersede(_))
    }

    /// A duplicate reuses an existing open record, so no create event is due.
    pub fn opened_or_changed_record(&self) -> bool {
        !matches!(self, Self::Duplicate(_))
    }
}

/// A push-eligible request. `scope` and `event_key` are daemon-derived; the
/// caller cannot opt out of either.
pub struct PushRequest {
    pub scope: String,
    pub level: NotifyLevel,
    pub event_key: String,
    pub title: String,
    pub body: Option<String>,
    pub severity: NotificationSeverity,
    pub provenance: NotificationProvenance,
    pub per_scope_limit: u32,
}

struct PushSlot {
    event_key: String,
    notification_id: String,
}

pub struct NotificationStore {
    notifications: Vec<Notification>,
    provider_updates: HashMap<String, String>,
    max_notifications: usize,
    /// One open coalescing slot per (scope, level). Closed on read, clear,
    /// eviction or tab close — never by elapsed time.
    push_slots: HashMap<(String, NotifyLevel), PushSlot>,
    /// Lifetime push count per scope. A count, so no clock can advance it.
    push_counts: HashMap<String, u32>,
    /// Live identities of unread records: the same message twice opens one.
    deliveries: DeliveryLedger,
    /// Standing conditions already announced, so re-observation stays quiet.
    conditions: ConditionLedger,
}

impl NotificationStore {
    pub fn new(max: usize) -> Self {
        Self {
            notifications: Vec::new(),
            provider_updates: HashMap::new(),
            max_notifications: max,
            push_slots: HashMap::new(),
            push_counts: HashMap::new(),
            deliveries: DeliveryLedger::default(),
            conditions: ConditionLedger::default(),
        }
    }

    /// Add a notification. Returns the notification ID.
    pub fn add(
        &mut self,
        title: String,
        body: Option<String>,
        severity: NotificationSeverity,
        tab_id: Option<String>,
        workspace_id: Option<String>,
    ) -> String {
        self.add_ext(
            title,
            body,
            severity,
            NotificationProvenance {
                tab_id,
                workspace_id,
                ..Default::default()
            },
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_ext(
        &mut self,
        title: String,
        body: Option<String>,
        severity: NotificationSeverity,
        provenance: NotificationProvenance,
        actions: Option<Vec<NotificationAction>>,
        sticky: Option<bool>,
        level: Option<NotifyLevel>,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Send-time capture: the asking tab is read from the provenance this
        // record is being created with, never resolved when an answer is tapped.
        let answer = answer::slot_for(actions.as_deref(), provenance.tab_id.as_deref());
        let notification = Notification {
            id: id.clone(),
            tab_id: provenance.tab_id,
            workspace_id: provenance.workspace_id,
            tab_title: provenance.tab_title,
            peer_name: provenance.peer_name,
            title,
            body,
            severity,
            level,
            timestamp,
            read: false,
            actions,
            answer,
            sticky,
            push_failed: None,
        };

        self.notifications.push(notification);

        while self.notifications.len() > self.max_notifications {
            let evicted = self.notifications.remove(0);
            self.close_push_slot_for(&evicted.id);
            self.deliveries.release(&evicted.id);
        }

        id
    }

    /// Add a notification unless an identical one is already live. Identity is
    /// derived here, so no caller can opt out of it.
    #[allow(clippy::too_many_arguments)]
    pub fn add_deduped(
        &mut self,
        title: String,
        body: Option<String>,
        severity: NotificationSeverity,
        provenance: NotificationProvenance,
        actions: Option<Vec<NotificationAction>>,
        sticky: Option<bool>,
        level: Option<NotifyLevel>,
    ) -> DeliveryAdmission {
        let identity = NotificationIdentity::of(&provenance, &severity, &title, body.as_deref());
        if let Some(id) = self.deliveries.existing(&identity) {
            return DeliveryAdmission::Duplicate(id);
        }
        let id = self.add_ext(title, body, severity, provenance, actions, sticky, level);
        self.deliveries.bind(identity, id.clone());
        DeliveryAdmission::Deliver(id)
    }

    /// Observe a condition that stays true. Only the absent -> present edge is
    /// news; every later observation of the same condition is the same news.
    pub fn observe_condition_present(
        &mut self,
        scope: &str,
        condition: &str,
    ) -> ConditionTransition {
        self.conditions.observe_present(scope, condition)
    }

    /// The condition resolved, so a later recurrence is a fresh transition.
    pub fn observe_condition_absent(&mut self, scope: &str, condition: &str) {
        self.conditions.observe_absent(scope, condition);
    }

    /// Admit a push-eligible notification. Dedup and coalescing are decided
    /// here so no caller can opt out of them.
    pub fn add_push(&mut self, request: PushRequest) -> PushAdmission {
        let slot_key = (request.scope.clone(), request.level);
        let open = self.push_slots.get(&slot_key).map(|slot| {
            (
                slot.event_key == request.event_key,
                slot.notification_id.clone(),
            )
        });

        if let Some((true, id)) = &open {
            return PushAdmission::Duplicate(id.clone());
        }

        let PushRequest {
            scope,
            level,
            event_key,
            title,
            body,
            severity,
            provenance,
            per_scope_limit,
        } = request;

        let reused = open.and_then(|(_, id)| {
            self.rewrite_in_place(
                &id,
                title.clone(),
                body.clone(),
                severity.clone(),
                &provenance,
                level,
            )
            .then_some(id)
        });
        let is_supersede = reused.is_some();
        let id = match reused {
            Some(id) => id,
            None => self.add_ext(title, body, severity, provenance, None, None, Some(level)),
        };

        self.push_slots.insert(
            slot_key,
            PushSlot {
                event_key,
                notification_id: id.clone(),
            },
        );

        let count = self.push_counts.entry(scope).or_insert(0);
        *count = count.saturating_add(1);
        if *count > per_scope_limit {
            return PushAdmission::Suppressed(id);
        }
        if is_supersede {
            PushAdmission::Supersede(id)
        } else {
            PushAdmission::Deliver(id)
        }
    }

    /// Content-only rewrite: the coalesced record keeps its id, position and
    /// opening timestamp so it reads as one event, not a new one.
    #[allow(clippy::too_many_arguments)]
    fn rewrite_in_place(
        &mut self,
        id: &str,
        title: String,
        body: Option<String>,
        severity: NotificationSeverity,
        provenance: &NotificationProvenance,
        level: NotifyLevel,
    ) -> bool {
        let Some(target) = self.notifications.iter_mut().find(|n| n.id == id) else {
            return false;
        };
        target.title = title;
        target.body = body;
        target.severity = severity;
        target.level = Some(level);
        target.tab_id = provenance.tab_id.clone();
        target.tab_title = provenance.tab_title.clone();
        target.workspace_id = provenance.workspace_id.clone();
        target.peer_name = provenance.peer_name.clone();
        target.read = false;
        target.push_failed = None;
        true
    }

    /// Record that the push sink refused this notification. The caller's id is
    /// unaffected; delivery failure never travels back to the agent.
    pub fn mark_push_failed(&mut self, id: &str) -> bool {
        match self.notifications.iter_mut().find(|n| n.id == id) {
            Some(target) => {
                target.push_failed = Some(true);
                true
            }
            None => false,
        }
    }

    /// Close every coalescing boundary owned by a closing tab. Tab ids are not
    /// reused, so dropping the counter cannot reset a live scope's ceiling.
    pub fn close_push_scope(&mut self, scope: &str) {
        self.push_slots
            .retain(|(slot_scope, _), _| slot_scope != scope);
        self.push_counts.remove(scope);
        self.deliveries.release_scope(scope);
        self.conditions.close_scope(scope);
    }

    fn close_push_slot_for(&mut self, id: &str) {
        self.push_slots.retain(|_, slot| slot.notification_id != id);
    }

    pub fn add_provider_update(
        &mut self,
        prompt_id: String,
        title: String,
        body: String,
        actions: Vec<NotificationAction>,
        provenance: NotificationProvenance,
    ) -> DedupAdmission {
        if let Some(id) = self.provider_updates.get(&prompt_id) {
            return DedupAdmission::Duplicate(id.clone());
        }
        let id = self.add_ext(
            title,
            Some(body),
            NotificationSeverity::Info,
            provenance,
            Some(actions),
            Some(true),
            None,
        );
        self.provider_updates.insert(prompt_id, id.clone());
        DedupAdmission::Added(id)
    }

    pub fn resolve_provider_update(&mut self, prompt_id: &str) -> Option<String> {
        let id = self.provider_updates.remove(prompt_id)?;
        self.clear(Some(&id));
        Some(id)
    }

    pub fn list(&self, unread_only: bool) -> Vec<&Notification> {
        if unread_only {
            self.notifications.iter().filter(|n| !n.read).collect()
        } else {
            self.notifications.iter().collect()
        }
    }

    pub fn mark_read(&mut self, id: &str) -> bool {
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) {
            n.read = true;
            self.close_push_slot_for(id);
            self.deliveries.release(id);
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self, id: Option<&str>) {
        match id {
            Some(id) => {
                self.notifications.retain(|n| n.id != id);
                self.provider_updates.retain(|_, value| value != id);
                self.close_push_slot_for(id);
                self.deliveries.release(id);
            }
            None => {
                self.notifications.clear();
                self.provider_updates.clear();
                self.push_slots.clear();
                self.deliveries.clear();
            }
        }
    }

    /// The one answer this notification may hold, and the tab it was asked
    /// from. Read-only: recording goes through `record_answer`.
    pub fn answer_slot(&self, id: &str) -> Option<&AnswerSlot> {
        self.notifications
            .iter()
            .find(|n| n.id == id)?
            .answer
            .as_ref()
    }

    /// The level the caller declared for this record, if any. Read-only: the
    /// emitted payload reads this back so it never drifts from what is stored.
    pub fn level(&self, id: &str) -> Option<NotifyLevel> {
        self.notifications.iter().find(|n| n.id == id)?.level
    }

    /// Record the user's choice. The invariant lives here, not in any surface:
    /// the same question renders in more than one place, and a per-surface
    /// guard would let it take two answers.
    pub fn record_answer(&mut self, id: &str, label: &str) -> AnswerAdmission {
        answer::record(&mut self.notifications, id, label)
    }

    /// What one record owes the tab that asked. Read-only: the send itself
    /// happens outside the guard, through the existing envelope path.
    pub fn answer_delivery_plan(&self, id: &str) -> AnswerDeliveryPlan {
        answer_delivery::plan(&self.notifications, id)
    }

    /// The standing answer reached the asking tab.
    pub fn mark_answer_delivered(&mut self, id: &str) -> bool {
        answer_delivery::mark_delivered(&mut self.notifications, id)
    }

    /// The tab that asked has closed. Its unanswered questions become visibly
    /// undeliverable; answers already recorded stay exactly as they were.
    pub fn forget_answer_tab(&mut self, tab_id: &str) {
        answer::forget_tab(&mut self.notifications, tab_id);
    }

    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_notification() {
        let mut store = NotificationStore::new(100);
        let id = store.add("Test".into(), None, NotificationSeverity::Info, None, None);
        assert!(!id.is_empty());
        assert_eq!(store.list(false).len(), 1);
    }

    #[test]
    fn test_notification_defaults_unread() {
        let mut store = NotificationStore::new(100);
        store.add("Test".into(), None, NotificationSeverity::Info, None, None);
        assert_eq!(store.unread_count(), 1);
        assert_eq!(store.list(true).len(), 1);
    }

    #[test]
    fn test_mark_read() {
        let mut store = NotificationStore::new(100);
        let id = store.add("Test".into(), None, NotificationSeverity::Info, None, None);
        assert!(store.mark_read(&id));
        assert_eq!(store.unread_count(), 0);
        assert_eq!(store.list(true).len(), 0);
        assert_eq!(store.list(false).len(), 1);
    }

    #[test]
    fn test_mark_read_nonexistent() {
        let mut store = NotificationStore::new(100);
        assert!(!store.mark_read("nonexistent"));
    }

    #[test]
    fn test_clear_specific() {
        let mut store = NotificationStore::new(100);
        let id1 = store.add("A".into(), None, NotificationSeverity::Info, None, None);
        let _id2 = store.add("B".into(), None, NotificationSeverity::Info, None, None);
        store.clear(Some(&id1));
        assert_eq!(store.list(false).len(), 1);
        assert_eq!(store.list(false)[0].title, "B");
    }

    #[test]
    fn test_clear_all() {
        let mut store = NotificationStore::new(100);
        store.add("A".into(), None, NotificationSeverity::Info, None, None);
        store.add("B".into(), None, NotificationSeverity::Info, None, None);
        store.clear(None);
        assert_eq!(store.list(false).len(), 0);
    }

    #[test]
    fn test_eviction() {
        let mut store = NotificationStore::new(3);
        store.add("A".into(), None, NotificationSeverity::Info, None, None);
        store.add("B".into(), None, NotificationSeverity::Info, None, None);
        store.add("C".into(), None, NotificationSeverity::Info, None, None);
        store.add("D".into(), None, NotificationSeverity::Info, None, None);
        assert_eq!(store.list(false).len(), 3);
        let titles: Vec<&str> = store.list(false).iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titles, vec!["B", "C", "D"]);
    }

    #[test]
    fn test_notification_severities() {
        let mut store = NotificationStore::new(100);
        store.add("Info".into(), None, NotificationSeverity::Info, None, None);
        store.add(
            "Warn".into(),
            None,
            NotificationSeverity::Warning,
            None,
            None,
        );
        store.add("Err".into(), None, NotificationSeverity::Error, None, None);
        let severities: Vec<&NotificationSeverity> =
            store.list(false).iter().map(|n| &n.severity).collect();
        assert_eq!(
            severities,
            vec![
                &NotificationSeverity::Info,
                &NotificationSeverity::Warning,
                &NotificationSeverity::Error
            ]
        );
    }

    #[test]
    fn test_notification_with_body_and_tab() {
        let mut store = NotificationStore::new(100);
        store.add(
            "Title".into(),
            Some("Body text".into()),
            NotificationSeverity::Info,
            Some("tab-1".into()),
            Some("ws-1".into()),
        );
        let n = &store.list(false)[0];
        assert_eq!(n.title, "Title");
        assert_eq!(n.body.as_deref(), Some("Body text"));
        assert_eq!(n.tab_id.as_deref(), Some("tab-1"));
        assert_eq!(n.workspace_id.as_deref(), Some("ws-1"));
    }

    #[test]
    fn provider_update_notification_is_actionable_deduplicated_and_resolvable() {
        let mut store = NotificationStore::new(100);
        let actions = vec![NotificationAction::Invoke {
            label: "Update".into(),
            command: codeterm_types::notification::DaemonCommand::RespondProviderUpdatePrompt,
            args: None,
        }];
        let provenance = NotificationProvenance {
            tab_id: Some("tab-1".into()),
            tab_title: Some("Build".into()),
            workspace_id: Some("workspace-1".into()),
            peer_name: None,
        };
        let first = store.add_provider_update(
            "prompt-1".into(),
            "Update available".into(),
            "Install separately".into(),
            actions.clone(),
            provenance.clone(),
        );
        let second = store.add_provider_update(
            "prompt-1".into(),
            "Duplicate".into(),
            "Duplicate".into(),
            actions.clone(),
            provenance,
        );
        let id = match first {
            DedupAdmission::Added(id) => id,
            _ => panic!("first provider update must be added"),
        };
        assert_eq!(second, DedupAdmission::Duplicate(id.clone()));
        assert_eq!(store.list(false).len(), 1);
        assert_eq!(store.list(false)[0].actions, Some(actions));
        assert_eq!(store.resolve_provider_update("prompt-1"), Some(id));
        assert!(store.list(false).is_empty());
    }

    /// Every test here is synchronous and clock-free by construction: needing a
    /// runtime or a time advance would prove a timer, not the mechanism.
    mod notification_admission {
        use super::*;
        use crate::notification::push;
        use std::collections::HashSet;

        const GENEROUS_LIMIT: u32 = 50;

        fn provenance_for(tab_id: &str) -> NotificationProvenance {
            NotificationProvenance {
                tab_id: Some(tab_id.to_string()),
                tab_title: Some("Worker lane".into()),
                workspace_id: Some("ws_lanes".into()),
                peer_name: None,
            }
        }

        fn request(
            tab_id: &str,
            level: NotifyLevel,
            supplied_key: Option<&str>,
            title: &str,
            body: &str,
            per_scope_limit: u32,
        ) -> PushRequest {
            let provenance = provenance_for(tab_id);
            PushRequest {
                scope: push::scope_key(&provenance),
                level,
                event_key: push::event_key(supplied_key, title),
                title: title.to_string(),
                body: Some(body.to_string()),
                severity: NotificationSeverity::Info,
                provenance,
                per_scope_limit,
            }
        }

        #[test]
        fn same_event_key_while_unread_is_a_duplicate() {
            const KEY: &str = "release-build";
            const FIRST_TITLE: &str = "Build started";
            const SECOND_TITLE: &str = "Build finished";
            const FIRST_BODY: &str = "compiling the workspace";
            const SECOND_BODY: &str = "artifacts are on disk";
            assert_ne!(
                FIRST_TITLE, SECOND_TITLE,
                "the two calls must differ in text so a text-hash dedup cannot pass this"
            );
            assert_ne!(FIRST_BODY, SECOND_BODY);

            let mut store = NotificationStore::new(10);
            let first = store.add_push(request(
                "tab_a",
                NotifyLevel::Verbose,
                Some(KEY),
                FIRST_TITLE,
                FIRST_BODY,
                GENEROUS_LIMIT,
            ));
            let second = store.add_push(request(
                "tab_a",
                NotifyLevel::Verbose,
                Some(KEY),
                SECOND_TITLE,
                SECOND_BODY,
                GENEROUS_LIMIT,
            ));

            assert!(matches!(first, PushAdmission::Deliver(_)));
            assert_eq!(second, PushAdmission::Duplicate(first.id().to_string()));
            assert!(!second.should_push());
            assert!(!second.opened_or_changed_record());
            assert_eq!(store.list(false).len(), 1, "no second record is created");
            assert_eq!(
                store.list(false)[0].title,
                FIRST_TITLE,
                "a duplicate leaves the open record untouched"
            );
        }

        #[test]
        fn reading_the_record_closes_the_boundary_for_the_same_key() {
            const KEY: &str = "release-build";
            let mut store = NotificationStore::new(10);
            let first = store.add_push(request(
                "tab_a",
                NotifyLevel::Verbose,
                Some(KEY),
                "Build started",
                "one",
                GENEROUS_LIMIT,
            ));
            assert!(store.mark_read(first.id()));

            let after_read = store.add_push(request(
                "tab_a",
                NotifyLevel::Verbose,
                Some(KEY),
                "Build started again",
                "two",
                GENEROUS_LIMIT,
            ));
            assert!(
                matches!(after_read, PushAdmission::Deliver(_)),
                "read closes the coalescing boundary — no timer does"
            );
            assert_ne!(after_read.id(), first.id());
            assert_eq!(store.list(false).len(), 2);
        }

        #[test]
        fn different_event_key_supersedes_in_place() {
            let mut store = NotificationStore::new(10);
            let first = store.add_push(request(
                "tab_a",
                NotifyLevel::Verbose,
                Some("step-1"),
                "Step one done",
                "first body",
                GENEROUS_LIMIT,
            ));
            let second = store.add_push(request(
                "tab_a",
                NotifyLevel::Verbose,
                Some("step-2"),
                "Step two done",
                "second body",
                GENEROUS_LIMIT,
            ));

            assert!(matches!(second, PushAdmission::Supersede(_)));
            assert!(second.should_push());
            assert_eq!(
                second.id(),
                first.id(),
                "in place means the same record id, not a new one"
            );
            let listed = store.list(false);
            assert_eq!(listed.len(), 1, "the inbox does not grow");
            assert_eq!(listed[0].title, "Step two done");
            assert_eq!(listed[0].body.as_deref(), Some("second body"));
            assert!(!listed[0].read);
        }

        #[test]
        fn per_tab_ceiling_suppresses_novel_event_keys() {
            const LIMIT: u32 = 3;
            let mut store = NotificationStore::new(10);
            let mut keys: Vec<String> = Vec::new();
            let mut outcomes: Vec<PushAdmission> = Vec::new();

            for step in 0..(LIMIT + 2) {
                let key = format!("novel-event-{step}");
                keys.push(key.clone());
                outcomes.push(store.add_push(request(
                    "tab_a",
                    NotifyLevel::Verbose,
                    Some(&key),
                    &format!("Progress {step}"),
                    "body",
                    LIMIT,
                )));
            }

            let unique: HashSet<&String> = keys.iter().collect();
            assert_eq!(
                unique.len(),
                keys.len(),
                "every key is novel, so the dedup branch cannot absorb any of them"
            );
            assert!(
                !outcomes
                    .iter()
                    .any(|o| matches!(o, PushAdmission::Duplicate(_))),
                "no admission may take the dedup branch in this fixture"
            );
            assert!(
                outcomes[..LIMIT as usize].iter().all(|o| o.should_push()),
                "everything under the ceiling still pushes"
            );
            assert!(
                outcomes[LIMIT as usize..]
                    .iter()
                    .all(|o| matches!(o, PushAdmission::Suppressed(_)) && !o.should_push()),
                "novel keys past the ceiling are suppressed"
            );
            assert_eq!(
                store.list(false).len(),
                1,
                "the durable record still lands and stays coalesced"
            );
        }

        #[test]
        fn clearing_the_inbox_does_not_reset_the_lifetime_ceiling() {
            const LIMIT: u32 = 1;
            let mut store = NotificationStore::new(10);
            let first = store.add_push(request(
                "tab_a",
                NotifyLevel::Verbose,
                Some("event-1"),
                "One",
                "body",
                LIMIT,
            ));
            assert!(first.should_push());
            store.clear(None);

            let after_clear = store.add_push(request(
                "tab_a",
                NotifyLevel::Verbose,
                Some("event-2"),
                "Two",
                "body",
                LIMIT,
            ));
            assert!(
                matches!(after_clear, PushAdmission::Suppressed(_)),
                "the ceiling is a lifetime count, so clearing cannot buy more pushes"
            );
        }

        #[test]
        fn closing_the_tab_scope_releases_its_slot_and_counter() {
            const LIMIT: u32 = 1;
            let mut store = NotificationStore::new(10);
            let scope = push::scope_key(&provenance_for("tab_a"));
            store.add_push(request(
                "tab_a",
                NotifyLevel::Verbose,
                Some("event-1"),
                "One",
                "body",
                LIMIT,
            ));
            store.close_push_scope(&scope);

            let reopened = store.add_push(request(
                "tab_a",
                NotifyLevel::Verbose,
                Some("event-2"),
                "Two",
                "body",
                LIMIT,
            ));
            assert!(matches!(reopened, PushAdmission::Deliver(_)));
            assert_eq!(store.list(false).len(), 2);
        }

        #[test]
        fn missing_event_key_still_dedups_by_derived_identity() {
            let mut store = NotificationStore::new(10);
            let first = store.add_push(request(
                "tab_a",
                NotifyLevel::ActionNeeded,
                None,
                "Approval required",
                "first body",
                GENEROUS_LIMIT,
            ));
            let second = store.add_push(request(
                "tab_a",
                NotifyLevel::ActionNeeded,
                None,
                "Approval required",
                "second body",
                GENEROUS_LIMIT,
            ));

            assert_eq!(
                second,
                PushAdmission::Duplicate(first.id().to_string()),
                "omitting the key derives one, so dedup is never opt-in"
            );
            assert_eq!(store.list(false).len(), 1);
        }

        #[test]
        fn levels_and_scopes_do_not_share_a_slot() {
            let mut store = NotificationStore::new(10);
            let verbose = store.add_push(request(
                "tab_a",
                NotifyLevel::Verbose,
                Some("shared"),
                "A",
                "body",
                GENEROUS_LIMIT,
            ));
            let action_needed = store.add_push(request(
                "tab_a",
                NotifyLevel::ActionNeeded,
                Some("shared"),
                "B",
                "body",
                GENEROUS_LIMIT,
            ));
            let other_tab = store.add_push(request(
                "tab_b",
                NotifyLevel::Verbose,
                Some("shared"),
                "C",
                "body",
                GENEROUS_LIMIT,
            ));

            assert!(matches!(verbose, PushAdmission::Deliver(_)));
            assert!(matches!(action_needed, PushAdmission::Deliver(_)));
            assert!(matches!(other_tab, PushAdmission::Deliver(_)));
            assert_eq!(store.list(false).len(), 3);
        }

        #[test]
        fn a_failed_push_marks_the_record_without_removing_it() {
            let mut store = NotificationStore::new(10);
            let admitted = store.add_push(request(
                "tab_a",
                NotifyLevel::Failure,
                Some("event-1"),
                "Terminal failure",
                "body",
                GENEROUS_LIMIT,
            ));
            assert!(store.mark_push_failed(admitted.id()));
            assert!(!store.mark_push_failed("never-existed"));

            let listed = store.list(false);
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].id, admitted.id());
            assert_eq!(listed[0].push_failed, Some(true));
        }
    }

    /// Every construction path that can carry a level writes it onto the
    /// record. Payload assembly is covered in `service::notification`.
    mod notification_level_field {
        use super::*;
        use crate::notification::push;

        #[test]
        fn add_ext_stores_the_level_independent_of_severity() {
            let mut store = NotificationStore::new(10);
            let id = store.add_ext(
                "Which branch?".into(),
                None,
                NotificationSeverity::Info,
                NotificationProvenance::default(),
                None,
                None,
                Some(NotifyLevel::ActionNeeded),
            );
            let listed = store.list(false);
            assert_eq!(listed[0].id, id);
            assert_eq!(listed[0].severity, NotificationSeverity::Info);
            assert_eq!(listed[0].level, Some(NotifyLevel::ActionNeeded));
        }

        #[test]
        fn add_ext_with_no_level_leaves_it_absent() {
            let mut store = NotificationStore::new(10);
            store.add_ext(
                "Still working".into(),
                None,
                NotificationSeverity::Info,
                NotificationProvenance::default(),
                None,
                None,
                None,
            );
            assert_eq!(store.list(false)[0].level, None);
        }

        #[test]
        fn add_deduped_carries_a_declared_level_onto_the_record() {
            let mut store = NotificationStore::new(10);
            let admission = store.add_deduped(
                "Which branch?".into(),
                None,
                NotificationSeverity::Info,
                NotificationProvenance::default(),
                None,
                None,
                Some(NotifyLevel::ActionNeeded),
            );
            let id = match admission {
                DeliveryAdmission::Deliver(id) => id,
                DeliveryAdmission::Duplicate(_) => panic!("first call must not dedupe"),
            };
            assert_eq!(store.level(&id), Some(NotifyLevel::ActionNeeded));
        }

        #[test]
        fn add_push_writes_the_request_level_on_both_fresh_and_superseded_records() {
            let mut store = NotificationStore::new(10);
            let provenance = NotificationProvenance {
                tab_id: Some("tab_a".into()),
                ..Default::default()
            };
            let fresh = store.add_push(PushRequest {
                scope: push::scope_key(&provenance),
                level: NotifyLevel::Failure,
                event_key: push::event_key(Some("step-1"), "Step one"),
                title: "Step one".into(),
                body: None,
                severity: NotificationSeverity::Info,
                provenance: provenance.clone(),
                per_scope_limit: 50,
            });
            assert_eq!(store.level(fresh.id()), Some(NotifyLevel::Failure));

            let superseded = store.add_push(PushRequest {
                scope: push::scope_key(&provenance),
                level: NotifyLevel::Failure,
                event_key: push::event_key(Some("step-2"), "Step two"),
                title: "Step two".into(),
                body: None,
                severity: NotificationSeverity::Info,
                provenance,
                per_scope_limit: 50,
            });
            assert_eq!(superseded.id(), fresh.id(), "supersede reuses the record");
            assert_eq!(store.level(superseded.id()), Some(NotifyLevel::Failure));
        }
    }
}
