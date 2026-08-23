//! `notify_gate` — the desktop gate proved at the same fail-closed standard the
//! push channel already meets.

use super::*;
use crate::notification::push;
use codeterm_types::config::schema::{AgentCategoryChoice, NotificationConfig};

/// Reads as the question the gate answers. Production reaches the sink by
/// holding the pass itself, so this stays a test-side convenience.
trait ReachesSink {
    fn reaches_sink(self) -> bool;
}

impl ReachesSink for DesktopAdmission {
    fn reaches_sink(self) -> bool {
        self.pass().is_some()
    }
}

fn default_policy() -> DesktopPolicy {
    DesktopPolicy::from_config(&NotificationConfig::default())
}

fn policy_with(agent_categories: Vec<AgentCategoryChoice>) -> DesktopPolicy {
    DesktopPolicy::from_config(&NotificationConfig {
        agent_categories,
        ..NotificationConfig::default()
    })
}

fn choice(category: &str, enabled: bool) -> AgentCategoryChoice {
    AgentCategoryChoice {
        category: category.to_string(),
        enabled,
    }
}

#[test]
fn notify_gate_uncategorised_never_reaches_the_desktop_sink() {
    let policy = default_policy();
    assert!(
        policy.enabled,
        "the fixture keeps desktop notifications on, so only the missing category can refuse"
    );
    assert!(
        !admit(None, &policy).reaches_sink(),
        "an uncategorised notification fails closed"
    );
    assert_eq!(admit(None, &policy), DesktopAdmission::Uncategorised);

    for category in NotifyCategory::ALL {
        assert!(
            admit(Some(category), &policy).reaches_sink(),
            "{} is an allowed category and reaches the sink",
            category.as_str()
        );
    }
}

#[test]
fn notify_gate_push_and_desktop_read_one_category_definition() {
    let policy = default_policy();
    for category in NotifyCategory::ALL {
        let wire = category.as_str();
        assert_eq!(
            NotifyCategory::parse(wire),
            Some(category),
            "the push transport parses {wire} from the shared definition"
        );
        assert!(
            policy.allows(category),
            "{wire} is valid for the push sink, so it is valid for the desktop sink"
        );
    }

    let shipped: Vec<&str> = NotificationConfig::default()
        .agent_categories
        .iter()
        .map(|row| row.category.as_str())
        .collect();
    assert_eq!(
        shipped,
        NotifyCategory::wire_values(),
        "the shipped desktop rows are the shared definition, not a second hand-written list"
    );
    assert_eq!(
        policy.allowed.len(),
        NotifyCategory::ALL.len(),
        "neither sink knows a category the other does not"
    );
}

#[test]
fn notify_gate_user_disabled_categories_do_not_reach_the_sink() {
    let policy = policy_with(vec![
        choice(NotifyCategory::Milestone.as_str(), false),
        choice(NotifyCategory::Blocker.as_str(), false),
        choice(NotifyCategory::Failure.as_str(), true),
    ]);

    assert!(admit(Some(NotifyCategory::Failure), &policy).reaches_sink());
    assert_eq!(
        admit(Some(NotifyCategory::Milestone), &policy),
        DesktopAdmission::CategoryDisabled(NotifyCategory::Milestone),
        "a category the user removed is refused even though it is in the closed set"
    );
    assert!(!admit(Some(NotifyCategory::Blocker), &policy).reaches_sink());

    let none_allowed = policy_with(Vec::new());
    for category in NotifyCategory::ALL {
        assert!(!admit(Some(category), &none_allowed).reaches_sink());
    }
}

#[test]
fn notify_gate_master_toggle_off_refuses_every_category() {
    let policy = DesktopPolicy::from_config(&NotificationConfig {
        desktop: false,
        ..NotificationConfig::default()
    });
    for category in NotifyCategory::ALL {
        assert_eq!(admit(Some(category), &policy), DesktopAdmission::DesktopOff);
    }
    assert_eq!(admit(None, &policy), DesktopAdmission::Uncategorised);
}

#[test]
fn notify_gate_unknown_config_entries_are_dropped_not_passed_through() {
    let policy = policy_with(vec![
        choice("progress", true),
        choice("everything", true),
        choice("", true),
        choice(NotifyCategory::Blocker.as_str(), true),
    ]);
    assert_eq!(policy.allowed, [NotifyCategory::Blocker]);
    assert!(!admit(None, &policy).reaches_sink());
}

#[test]
fn notify_gate_silent_policy_admits_nothing() {
    let policy = DesktopPolicy::silent();
    assert!(!policy.enabled);
    assert!(policy.allowed.is_empty());
    for category in NotifyCategory::ALL {
        assert!(!admit(Some(category), &policy).reaches_sink());
    }
}

/// The push scope helper still reads the same provenance the desktop gate rides
/// alongside: two independent decisions on one record, not two records.
#[test]
fn notify_gate_composes_with_the_push_scope_without_sharing_a_decision() {
    let provenance = crate::notification::store::NotificationProvenance {
        tab_id: Some("tab_gate".into()),
        tab_title: Some("Worker lane".into()),
        workspace_id: Some("ws_lanes".into()),
        peer_name: None,
    };
    assert_eq!(push::scope_key(&provenance), "tab:tab_gate");
    assert!(admit(Some(NotifyCategory::Blocker), &default_policy()).reaches_sink());
}

/// Structural, not behavioural: the desktop sink is reachable only from the
/// explicit `codeterm notify` verb and two pre-existing app-lifecycle callers.
/// No code may decide on an agent's behalf that it wants to interrupt.
mod no_inference {
    use std::fs;
    use std::path::{Path, PathBuf};

    const SINK: &str = "_send_os_notification";

    /// The sink's own definition, the explicit notify verb guarded by the gate,
    /// and the updater reporting its own abort. Nothing else may name the toast.
    const SINK_CALLERS: [&str; 3] = [
        "api/os_notify.rs",
        "service/notification.rs",
        "update/failure_report.rs",
    ];

    /// Every file permitted to open an explicit notification record. Each is a
    /// transport for the `codeterm notify` verb; none observes agent output.
    const EXPLICIT_CREATORS: [&str; 4] = [
        "api/notification_routes.rs",
        "notification/mod.rs",
        "service/notification.rs",
        "socket/server/notification.rs",
    ];

    fn src_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    fn files_naming(needle: &str) -> Vec<String> {
        let root = src_root();
        let mut hits = Vec::new();
        collect(&root, &root, needle, &mut hits);
        hits.sort();
        hits
    }

    fn collect(root: &Path, dir: &Path, needle: &str, hits: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect(root, &path, needle, hits);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let Ok(source) = fs::read_to_string(&path) else {
                continue;
            };
            if source.contains(needle) {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                hits.push(relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"));
            }
        }
    }

    /// This test file names the sink in its own fixture, so it excludes itself.
    fn callers_of(needle: &str) -> Vec<String> {
        files_naming(needle)
            .into_iter()
            .filter(|file| file.as_str() != "notification/desktop/tests.rs")
            .collect()
    }

    #[test]
    fn notify_gate_desktop_sink_has_no_inference_path() {
        let mut allowed: Vec<String> = SINK_CALLERS.iter().map(|f| f.to_string()).collect();
        allowed.sort();
        assert_eq!(
            callers_of(SINK),
            allowed,
            "a new file naming the OS toast must be justified: the desktop sink is reached from the explicit verb, never from observing an agent"
        );
    }

    #[test]
    fn notify_gate_explicit_records_open_only_from_the_notify_verb() {
        let mut allowed: Vec<String> = EXPLICIT_CREATORS.iter().map(|f| f.to_string()).collect();
        allowed.sort();
        assert_eq!(
            callers_of("create_notification_ext"),
            allowed,
            "an explicit notification is opened by the notify verb's transports only"
        );
    }
}
