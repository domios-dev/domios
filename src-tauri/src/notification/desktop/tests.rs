//! `notify_gate` — the desktop gate proved at the same fail-closed standard the
//! push channel already meets.

use super::*;
use crate::notification::push;
use codeterm_types::config::schema::{AgentLevelChoice, NotificationConfig};

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

/// A user who switched the desktop surface on and opened every level's column.
/// The shipped default routes nothing here, so the gate's own behaviour has to
/// be asked of an explicit opt-in rather than of the defaults.
fn opted_in_policy() -> DesktopPolicy {
    DesktopPolicy::from_config(&opted_in_config())
}

fn opted_in_config() -> NotificationConfig {
    let mut cfg = NotificationConfig {
        desktop: true,
        ..NotificationConfig::default()
    };
    for row in &mut cfg.agent_levels {
        row.set(NotifyDestination::Desktop, true);
    }
    cfg
}

/// The surface is switched on here so the ROWS are what refuse. Inheriting the
/// shipped master switch would answer `DesktopOff` before a row was consulted,
/// and every per-level assertion below would pass for the wrong reason.
fn policy_with(agent_levels: Vec<AgentLevelChoice>) -> DesktopPolicy {
    DesktopPolicy::from_config(&NotificationConfig {
        desktop: true,
        agent_levels,
        ..NotificationConfig::default()
    })
}

fn choice(level: &str, enabled: bool) -> AgentLevelChoice {
    let mut choice = AgentLevelChoice::open(level, &[]);
    choice.set(
        codeterm_types::notification::NotifyDestination::Desktop,
        enabled,
    );
    choice
}

#[test]
fn notify_gate_unlevelled_never_reaches_the_desktop_sink() {
    let policy = opted_in_policy();
    assert!(
        policy.enabled,
        "the fixture switched desktop notifications on, so only the missing level can refuse"
    );
    assert!(
        !admit(None, &policy).reaches_sink(),
        "an unlevelled notification fails closed"
    );
    assert_eq!(admit(None, &policy), DesktopAdmission::Unlevelled);

    for level in NotifyLevel::ALL {
        assert!(
            admit(Some(level), &policy).reaches_sink(),
            "{} is an allowed level and reaches the sink",
            level.as_str()
        );
    }
}

#[test]
fn notify_gate_push_and_desktop_read_one_level_definition() {
    let policy = opted_in_policy();
    for level in NotifyLevel::ALL {
        let wire = level.as_str();
        assert_eq!(
            NotifyLevel::parse(wire),
            Some(level),
            "the push transport parses {wire} from the shared definition"
        );
        assert!(
            policy.allows(level),
            "{wire} is valid for the push sink, so it is valid for the desktop sink"
        );
    }

    let cfg = NotificationConfig::default();
    let shipped: Vec<&str> = cfg
        .agent_levels
        .iter()
        .map(|row| row.level.as_str())
        .collect();
    assert_eq!(
        shipped,
        NotifyLevel::wire_values(),
        "the shipped desktop rows are the shared definition, not a second hand-written list"
    );
    assert_eq!(
        policy.allowed.len(),
        NotifyLevel::ALL.len(),
        "neither sink knows a level the other does not"
    );
}

#[test]
fn notify_gate_user_disabled_levels_do_not_reach_the_sink() {
    let policy = policy_with(vec![
        choice(NotifyLevel::Verbose.as_str(), false),
        choice(NotifyLevel::ActionNeeded.as_str(), false),
        choice(NotifyLevel::Failure.as_str(), true),
    ]);

    assert!(admit(Some(NotifyLevel::Failure), &policy).reaches_sink());
    assert_eq!(
        admit(Some(NotifyLevel::Verbose), &policy),
        DesktopAdmission::LevelDisabled(NotifyLevel::Verbose),
        "a level the user removed is refused even though it is in the closed set"
    );
    assert!(!admit(Some(NotifyLevel::ActionNeeded), &policy).reaches_sink());

    let none_allowed = policy_with(Vec::new());
    for level in NotifyLevel::ALL {
        assert!(!admit(Some(level), &none_allowed).reaches_sink());
    }
}

#[test]
fn notify_gate_master_toggle_off_refuses_every_level() {
    let policy = DesktopPolicy::from_config(&NotificationConfig {
        desktop: false,
        ..NotificationConfig::default()
    });
    for level in NotifyLevel::ALL {
        assert_eq!(admit(Some(level), &policy), DesktopAdmission::DesktopOff);
    }
    assert_eq!(admit(None, &policy), DesktopAdmission::Unlevelled);
}

/// The persisted-row decision: config rows are strings on disk, and this repo
/// ships no migration for the retired names. One unreadable row must cost that
/// row only — never the whole read.
#[test]
fn notify_level_unparseable_stored_row_is_dropped_and_the_rest_still_read() {
    let policy = policy_with(vec![
        choice(NotifyLevel::Verbose.as_str(), true),
        // Rows written by the retired taxonomy, plus outright garbage.
        choice("milestone", true),
        choice("blocker", true),
        choice("everything", true),
        choice("", true),
        choice(NotifyLevel::ActionNeeded.as_str(), true),
    ]);

    assert_eq!(
        policy.allowed,
        [NotifyLevel::Verbose, NotifyLevel::ActionNeeded],
        "every readable row survives; only the unreadable ones are dropped"
    );
    for surviving in [NotifyLevel::Verbose, NotifyLevel::ActionNeeded] {
        assert!(
            admit(Some(surviving), &policy).reaches_sink(),
            "{} still reaches the sink despite unreadable neighbours",
            surviving.as_str()
        );
    }
    assert!(
        !admit(Some(NotifyLevel::Failure), &policy).reaches_sink(),
        "a level no readable row named stays refused"
    );
    assert!(!admit(None, &policy).reaches_sink());
}

/// Every shipped list is `ALL` mapped, so no surface can drift from the set.
#[test]
fn notify_level_all_is_the_single_source_for_every_shipped_list() {
    let cfg = NotificationConfig::default();
    let shipped: Vec<String> = cfg
        .agent_levels
        .iter()
        .map(|row| row.level.clone())
        .collect();
    let derived: Vec<String> = NotifyLevel::ALL
        .into_iter()
        .map(|level| level.as_str().to_string())
        .collect();
    assert_eq!(
        shipped, derived,
        "the shipped rows are ALL mapped, not a second hand-written list"
    );
    assert_eq!(
        opted_in_policy().allowed.len(),
        NotifyLevel::ALL.len(),
        "the gate knows exactly the shared set"
    );
}

/// The rename is semantic, not cosmetic: the retired wire strings are rejected
/// at the transport boundary rather than quietly accepted.
#[test]
fn notify_level_wire_parse_rejects_the_retired_names() {
    use crate::api::notification_routes::parse_push_intent;

    for level in NotifyLevel::ALL {
        let intent = parse_push_intent(Some(level.as_str()), None)
            .unwrap_or_else(|e| panic!("{} must parse at the boundary: {e}", level.as_str()));
        assert_eq!(intent.level, Some(level));
    }
    for retired in ["milestone", "blocker", "progress", "Milestone"] {
        assert!(
            parse_push_intent(Some(retired), None).is_err(),
            "{retired} must be rejected on the wire, not passed through"
        );
    }
    assert_eq!(
        parse_push_intent(None, None)
            .expect("naming no level stays legal")
            .level,
        None,
        "omitting the level is still the fail-closed inbox path"
    );
}

#[test]
fn notify_gate_silent_policy_admits_nothing() {
    let policy = DesktopPolicy::silent();
    assert!(!policy.enabled);
    assert!(policy.allowed.is_empty());
    for level in NotifyLevel::ALL {
        assert!(!admit(Some(level), &policy).reaches_sink());
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
    assert!(admit(Some(NotifyLevel::ActionNeeded), &opted_in_policy()).reaches_sink());
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

    /// Every file permitted to open an explicit notification record. Three are
    /// transports for the `codeterm notify` verb; the other two are the daemon
    /// speaking for itself. NONE observes agent output, which is the line this
    /// gate actually holds — a record must come from a declared fact, never from
    /// reading what an agent printed.
    ///
    /// `service/integration_silence.rs` qualifies on that rule: it reads the
    /// stored consent flag and the provider's own install report, latches the
    /// condition so ten launches announce once, and reads no clock and no output.
    // Daemon notices the owner asked for: consent, model fallback, restart-required, update skipped.
    const EXPLICIT_CREATORS: [&str; 12] = [
        "api/notification_routes.rs",
        "commands/hooks.rs",
        "notification/mod.rs",
        "service/directory_trust.rs",
        "service/integration_silence.rs",
        "service/launch_readiness.rs",
        "service/notification.rs",
        "service/output/manual_update.rs",
        "service/provider_update.rs",
        "service/session_model.rs",
        "service/spawn_model.rs",
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
            // Without this the gate grows an allowlist entry every time a slice covers its own seam, and the list stops meaning what it says.
            let is_test_file = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "tests.rs" || name.ends_with("_tests.rs"));
            if is_test_file || path.components().any(|part| part.as_os_str() == "tests") {
                continue;
            }
            let Ok(source) = fs::read_to_string(&path) else {
                continue;
            };
            if source.contains(needle) {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                hits.push(
                    relative
                        .to_string_lossy()
                        .replace(std::path::MAIN_SEPARATOR, "/"),
                );
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
