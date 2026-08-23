//! `notify_gate` at the sink: what an agent's `codeterm notify` call actually
//! reaches, proved against the same fail-closed standard the push channel meets.

use super::*;
use crate::notification::store::NotificationStore;
use codeterm_types::config::schema::{AgentCategoryChoice, NotificationConfig};
use parking_lot::Mutex;
use std::sync::Arc;

struct Sinks {
    store: NotifState,
    native: Mutex<Vec<String>>,
    emitted: Mutex<Vec<serde_json::Value>>,
}

impl Sinks {
    fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(NotificationStore::new(10))),
            native: Mutex::new(Vec::new()),
            emitted: Mutex::new(Vec::new()),
        }
    }

    fn records(&self) -> usize {
        self.store.lock().list(false).len()
    }
}

fn policy_allowing(categories: &[NotifyCategory]) -> DesktopPolicy {
    DesktopPolicy::from_config(&NotificationConfig {
        agent_categories: NotifyCategory::ALL
            .into_iter()
            .map(|category| AgentCategoryChoice {
                category: category.as_str().to_string(),
                enabled: categories.contains(&category),
            })
            .collect(),
        ..NotificationConfig::default()
    })
}

/// One agent notification through the explicit verb's own code path.
fn notify(sinks: &Sinks, title: &str, desktop: DesktopDispatch) -> String {
    create_notification_with_delivery(
        NotificationAdmission::Explicit,
        &sinks.store,
        |title, _| sinks.native.lock().push(title.to_string()),
        title.to_string(),
        Some("agent body".into()),
        Some("info"),
        None,
        None,
        NotificationOrigin::default(),
        |_| None,
        PushDispatch::inert(),
        desktop,
        |payload| sinks.emitted.lock().push(payload),
    )
}

fn shipped(declared: Option<NotifyCategory>) -> DesktopDispatch {
    DesktopDispatch {
        declared,
        policy: DesktopPolicy::from_config(&NotificationConfig::default()),
    }
}

#[test]
fn notify_gate_allowed_category_reaches_the_desktop_sink() {
    let sinks = Sinks::new();
    for category in NotifyCategory::ALL {
        notify(&sinks, category.as_str(), shipped(Some(category)));
    }

    assert_eq!(
        sinks.native.lock().len(),
        NotifyCategory::ALL.len(),
        "every category of the closed set reaches the toast under the shipped policy"
    );
    assert_eq!(sinks.records(), NotifyCategory::ALL.len());
    assert_eq!(sinks.emitted.lock().len(), NotifyCategory::ALL.len());
}

#[test]
fn notify_gate_uncategorised_notification_never_reaches_the_desktop_sink() {
    let sinks = Sinks::new();
    let id = notify(&sinks, "step 4 of 9", shipped(None));

    assert!(
        sinks.native.lock().is_empty(),
        "an uncategorised agent notification fails closed at the desktop sink"
    );
    assert_eq!(
        sinks.records(),
        1,
        "the record still exists: the message is not interrupting, not erased"
    );
    assert_eq!(sinks.emitted.lock().len(), 1, "the bell route still fires");
    assert_eq!(sinks.emitted.lock()[0]["id"], id);
}

#[test]
fn notify_gate_user_disabled_category_keeps_the_record_but_not_the_toast() {
    let sinks = Sinks::new();
    let only_failures = |declared| DesktopDispatch {
        declared: Some(declared),
        policy: policy_allowing(&[NotifyCategory::Failure]),
    };

    notify(&sinks, "milestone", only_failures(NotifyCategory::Milestone));
    notify(&sinks, "blocker", only_failures(NotifyCategory::Blocker));
    assert!(
        sinks.native.lock().is_empty(),
        "categories the user turned off do not reach the sink"
    );

    notify(&sinks, "failure", only_failures(NotifyCategory::Failure));
    assert_eq!(sinks.native.lock().as_slice(), &["failure"]);
    assert_eq!(sinks.records(), 3, "every message kept its record");
}

#[test]
fn notify_gate_master_desktop_toggle_off_silences_every_category() {
    let sinks = Sinks::new();
    let cfg = NotificationConfig {
        desktop: false,
        ..NotificationConfig::default()
    };

    for category in NotifyCategory::ALL {
        notify(
            &sinks,
            category.as_str(),
            DesktopDispatch {
                declared: Some(category),
                policy: DesktopPolicy::from_config(&cfg),
            },
        );
    }

    assert!(sinks.native.lock().is_empty());
    assert_eq!(sinks.records(), NotifyCategory::ALL.len());
}

/// Negative control: the two routes that never toast today still never toast,
/// and their store behaviour is untouched by the gate.
#[test]
fn notify_gate_leaves_non_agent_routes_unchanged() {
    let sinks = Sinks::new();

    for source in ["service/spawn.rs", "cron/scheduler.rs"] {
        create_notification_with_delivery(
            NotificationAdmission::Diagnostic,
            &sinks.store,
            |title, _| sinks.native.lock().push(title.to_string()),
            format!("Diagnostic from {source}"),
            Some("warning detail".into()),
            Some("warning"),
            None,
            None,
            NotificationOrigin::default(),
            |_| None,
            PushDispatch::inert(),
            DesktopDispatch::silent(),
            |payload| sinks.emitted.lock().push(payload),
        );
    }
    assert_eq!(sinks.records(), 0, "diagnostics still bypass the inbox");
    assert!(sinks.native.lock().is_empty());
    assert!(sinks.emitted.lock().is_empty());

    let live_tab = |candidate: &str| {
        (candidate == "tab_update").then(|| NotificationProvenance {
            tab_id: Some("tab_update".into()),
            tab_title: Some("Codex".into()),
            workspace_id: Some("ws_main".into()),
            peer_name: None,
        })
    };
    let id = create_notification_with_delivery(
        NotificationAdmission::ProviderUpdate {
            prompt_id: "prompt_1".into(),
        },
        &sinks.store,
        |title, _| sinks.native.lock().push(title.to_string()),
        "Codex update available".into(),
        Some("1.2.3".into()),
        Some("info"),
        Some(serde_json::json!([])),
        Some(true),
        NotificationOrigin::local_tab("tab_update".into()),
        live_tab,
        PushDispatch::inert(),
        DesktopDispatch::silent(),
        |payload| sinks.emitted.lock().push(payload),
    );

    assert_eq!(sinks.records(), 1, "the in-app prompt is still stored");
    assert_eq!(sinks.emitted.lock().len(), 1, "and still emitted");
    assert_eq!(sinks.emitted.lock()[0]["id"], id);
    assert!(
        sinks.native.lock().is_empty(),
        "the prompt never raised a toast before the gate and still does not"
    );
}

/// Dedupe answers "is this new?", the gate answers "is this allowed?".
/// Neither answer stands in for the other.
#[test]
fn notify_gate_and_dedupe_are_separate_decisions() {
    let sinks = Sinks::new();

    let first = notify(&sinks, "approval needed", shipped(Some(NotifyCategory::Blocker)));
    let repeat = notify(&sinks, "approval needed", shipped(Some(NotifyCategory::Blocker)));
    assert_eq!(repeat, first, "dedupe kept one record for one message");
    assert_eq!(
        sinks.native.lock().len(),
        1,
        "allowed and new toasts once; allowed and repeated does not toast again"
    );

    let quiet = notify(&sinks, "step 4 of 9", shipped(None));
    assert_ne!(quiet, first, "a different message is a different record");
    assert_eq!(
        sinks.native.lock().len(),
        1,
        "new but not allowed adds a record without a toast"
    );
    assert_eq!(sinks.records(), 2);
}
