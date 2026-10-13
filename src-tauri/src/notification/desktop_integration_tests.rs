use codeterm_types::notification::NotifyLevel;

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

    const EXPLICIT_CREATORS: [&str; 3] = [
        "api/notification_routes.rs",
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
            // Without this the gate grows an allowlist entry every time a slice covers its own seam, and the list stops meaning what it says.
            let is_test_file = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "tests.rs" || name.ends_with("_tests.rs"));
            let is_test_module = path.components().any(|part| {
                let part = part.as_os_str().to_string_lossy();
                part == "tests" || part.ends_with("_tests")
            });
            if is_test_file || is_test_module {
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
            callers_of("create_agent_notification_reported"),
            allowed,
            "an explicit notification is opened by the notify verb's transports only"
        );
    }
}
