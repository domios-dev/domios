use super::*;

#[test]
fn child_environment_drops_the_daemon_candle_budget_and_keeps_user_values() {
    let edits = child_env_changes_with_budget(
        [("CANDLE_NUM_THREADS", "2"), ("KEEP", "user value")],
        &[],
        true,
    );
    assert!(edits.contains(&("CANDLE_NUM_THREADS".into(), None)));
    assert!(!edits.iter().any(|(key, _)| key == "KEEP"));
    let edits = child_env_changes_with_budget([("Candle_Num_Threads", "2")], &[], true);
    assert!(edits.contains(&("Candle_Num_Threads".into(), None)));
}

#[test]
fn child_environment_preserves_user_candle_values_without_host_injection() {
    for value in ["2", "8", ""] {
        let edits = child_env_changes_with_budget([("CANDLE_NUM_THREADS", value)], &[], false);
        assert!(!edits.iter().any(|(key, _)| key == "CANDLE_NUM_THREADS"));
    }
}

#[test]
fn child_environment_preserves_an_override_of_the_injected_candle_budget() {
    let edits = child_env_changes_with_budget([("CANDLE_NUM_THREADS", "8")], &[], true);
    assert!(!edits.iter().any(|(key, _)| key == "CANDLE_NUM_THREADS"));
    let edits = child_env_changes_with_budget(
        [("CANDLE_NUM_THREADS", "8")],
        &["CANDLE_NUM_THREADS"],
        false,
    );
    assert!(edits.contains(&("CANDLE_NUM_THREADS".into(), None)));
}

const APPDIR: &str = "/tmp/.mount_CodeTeXyz";

fn changes(vars: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
    let mut changes = appimage_env_changes(vars);
    changes.sort();
    changes
}

#[test]
fn appimage_cleanup_is_inert_outside_an_appimage() {
    let vars = [
        ("APPDIR", APPDIR),
        ("LD_LIBRARY_PATH", "/tmp/.mount_CodeTeXyz/usr/lib"),
    ];
    assert!(
        changes(&vars).is_empty(),
        "APPDIR alone is not proof of an AppImage"
    );
    assert!(changes(&[("PATH", "/usr/bin")]).is_empty());
}

#[test]
fn appimage_cleanup_drops_only_entries_inside_the_mounted_image() {
    let vars = [
        ("APPDIR", APPDIR),
        ("APPIMAGE", "/home/u/CodeTerm.AppImage"),
        ("ARGV0", "CodeTerm.AppImage"),
        ("OWD", "/home/u"),
        (
            "PATH",
            "/tmp/.mount_CodeTeXyz/usr/bin:/usr/local/bin:/usr/bin",
        ),
        (
            "LD_LIBRARY_PATH",
            "/tmp/.mount_CodeTeXyz/usr/lib:/tmp/.mount_CodeTeXyz/lib",
        ),
        (
            "GDK_PIXBUF_MODULE_FILE",
            "/tmp/.mount_CodeTeXyz/usr/lib/loaders.cache",
        ),
        (
            "XDG_DATA_DIRS",
            "/tmp/.mount_CodeTeXyz/usr/share:/usr/share",
        ),
        ("HOME", "/home/u"),
        ("BROWSER_URL", "http://localhost:3000"),
        ("SIBLING", "/tmp/.mount_CodeTeXyzOther/bin"),
    ];
    assert_eq!(
        changes(&vars),
        vec![
            ("APPDIR".to_string(), None),
            ("APPIMAGE".to_string(), None),
            ("ARGV0".to_string(), None),
            ("GDK_PIXBUF_MODULE_FILE".to_string(), None),
            ("LD_LIBRARY_PATH".to_string(), None),
            ("OWD".to_string(), None),
            (
                "PATH".to_string(),
                Some("/usr/local/bin:/usr/bin".to_string())
            ),
            ("XDG_DATA_DIRS".to_string(), Some("/usr/share".to_string())),
        ]
    );
}

#[cfg(windows)]
#[test]
fn windows_children_default_python_to_utf8_without_overriding_a_choice() {
    assert_eq!(
        child_env_changes([("PATH", "C:\\Windows")]),
        vec![("PYTHONUTF8".to_string(), Some("1".to_string()))]
    );
    assert!(child_env_changes([("PythonUtf8", "0")]).is_empty());
}
