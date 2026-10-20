use super::*;

fn keys() -> Vec<String> {
    ["projects", "/exact", "hasTrustDialogAccepted"]
        .map(str::to_string)
        .to_vec()
}

fn record(source: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("record.json");
    fs::write(&file, source).unwrap();
    (dir, file)
}

fn no_temps(dir: &Path) {
    assert_eq!(fs::read_dir(dir).unwrap().count(), 1);
}

#[test]
fn already_true_is_untouched_with_no_temporary_file() {
    let source = r#"{"projects":{"/exact":{"hasTrustDialogAccepted":true}}}"#;
    let (dir, file) = record(source);
    let before = fs::metadata(&file).unwrap();
    assert_eq!(grant(&file, &keys()), PreTrustOutcome::AlreadyTrusted);
    assert!(same_version(&before, &fs::metadata(&file).unwrap()));
    assert_eq!(fs::read_to_string(&file).unwrap(), source);
    no_temps(dir.path());
}

#[test]
fn false_changes_only_the_boolean_span() {
    let source = "{\n  \"large\": 12345678901234567890,\n  \"projects\": {\n    \"/exact\": {\n      \"history\": [\"quoted \\\" \\u0061\", {\"z\": 1, \"a\": 2}],\n      \"hasTrustDialogAccepted\": false,\n      \"after\": null\n    },\n    \"/other\": {\"hasTrustDialogAccepted\": false}\n  },\n  \"tail\": 2\n}\n";
    let (dir, file) = record(source);
    assert_eq!(grant(&file, &keys()), PreTrustOutcome::Granted);
    let after = fs::read_to_string(file).unwrap();
    let at = source.find("\"hasTrustDialogAccepted\": false").unwrap()
        + "\"hasTrustDialogAccepted\": ".len();
    assert_eq!(&source[..at], &after[..at]);
    assert_eq!(&source[at + 5..], &after[at + 4..]);
    assert_other_values_equal(source, &after);
    no_temps(dir.path());
}

fn assert_other_values_equal(before_source: &str, after_source: &str) {
    let before: serde_json::Value = serde_json::from_str(before_source).unwrap();
    let mut expected = before.clone();
    expected["projects"]["/exact"]["hasTrustDialogAccepted"] = serde_json::Value::Bool(true);
    let after: serde_json::Value = serde_json::from_str(after_source).unwrap();
    assert_eq!(expected, after);
    fn order(node: &Node) -> Vec<String> {
        node.members
            .as_ref()
            .map(|members| members.iter().map(|(key, _)| key.clone()).collect())
            .unwrap_or_default()
    }
    let before_node = Parser {
        source: before_source,
        at: 0,
    }
    .node(0)
    .unwrap();
    let after_node = Parser {
        source: after_source,
        at: 0,
    }
    .node(0)
    .unwrap();
    assert_eq!(order(&before_node), order(&after_node));
}

#[test]
fn missing_leaf_project_or_projects_are_inserted_with_two_space_indent() {
    for (source, expected) in [
        ("{}", "{\n  \"projects\": {\n    \"/exact\": {\n      \"hasTrustDialogAccepted\": true\n    }\n  }\n}"),
        ("{\n  \"projects\": {}\n}", "{\n  \"projects\": {\n    \"/exact\": {\n      \"hasTrustDialogAccepted\": true\n    }\n  }\n}"),
        ("{\n  \"projects\": {\n    \"/exact\": {}\n  }\n}", "{\n  \"projects\": {\n    \"/exact\": {\n      \"hasTrustDialogAccepted\": true\n    }\n  }\n}"),
    ] {
        let (dir, file) = record(source);
        assert_eq!(grant(&file, &keys()), PreTrustOutcome::Granted);
        assert_eq!(fs::read_to_string(file).unwrap(), expected);
        no_temps(dir.path());
    }
}

#[test]
fn insertion_preserves_all_unrelated_bytes_and_key_order() {
    let source = "{\n  \"z\": [1, 2],\n  \"projects\": {\n    \"/exact\": {\n      \"z\": \"\\u0061\",\n      \"a\": 9\n    },\n    \"/other\": {\"hasTrustDialogAccepted\": false}\n  },\n  \"a\": 0\n}";
    let (dir, file) = record(source);
    assert_eq!(grant(&file, &keys()), PreTrustOutcome::Granted);
    let after = fs::read_to_string(file).unwrap();
    let addition = ",\n      \"hasTrustDialogAccepted\": true";
    assert_eq!(after.replacen(addition, "", 1), source);
    assert_other_values_equal(source, &after);
    no_temps(dir.path());
}

#[test]
fn concurrent_change_retries_the_entire_edit_and_preserves_the_new_data() {
    let (dir, file) = record(r#"{"projects":{},"revision":0}"#);
    let mut attempts = 0;
    let outcome = grant_with(&file, &keys(), |attempt, file| {
        attempts += 1;
        if attempt == 0 {
            fs::write(file, r#"{"projects":{},"revision":100}"#).unwrap();
        }
    });
    assert_eq!(outcome, PreTrustOutcome::Granted);
    assert_eq!(attempts, 2);
    let after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
    assert_eq!(after["revision"], 100);
    assert_eq!(after["projects"]["/exact"]["hasTrustDialogAccepted"], true);
    no_temps(dir.path());
}

#[test]
fn persistent_change_fails_after_three_attempts_and_cleans_temps() {
    let (dir, file) = record("{}");
    let mut attempts = 0;
    let outcome = grant_with(&file, &keys(), |_, file| {
        attempts += 1;
        let mut source = fs::read_to_string(file).unwrap();
        source.push(' ');
        fs::write(file, source).unwrap();
    });
    assert!(matches!(outcome, PreTrustOutcome::Failed(_)));
    assert_eq!(attempts, MAX_ATTEMPTS);
    assert_eq!(fs::read_to_string(file).unwrap(), "{}   ");
    no_temps(dir.path());
}

#[test]
fn missing_invalid_non_regular_or_ambiguous_records_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        grant(&dir.path().join("missing"), &keys()),
        PreTrustOutcome::Skipped(_)
    ));
    assert!(matches!(
        grant(dir.path(), &keys()),
        PreTrustOutcome::Skipped(_)
    ));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    for source in [
        "bad json",
        "[]",
        r#"{"projects":null}"#,
        r#"{"projects":{},"projects":{}}"#,
        r#"{"projects":{"/exact":{"hasTrustDialogAccepted":null}}}"#,
    ] {
        let (dir, file) = record(source);
        assert!(matches!(grant(&file, &keys()), PreTrustOutcome::Skipped(_)));
        assert_eq!(fs::read_to_string(file).unwrap(), source);
        no_temps(dir.path());
    }
}

#[cfg(unix)]
#[test]
fn symlink_is_skipped_and_original_permissions_are_preserved() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (dir, file) = record("{}");
    let alias = dir.path().join("alias.json");
    symlink(&file, &alias).unwrap();
    assert!(matches!(
        grant(&alias, &keys()),
        PreTrustOutcome::Skipped(_)
    ));
    assert_eq!(fs::read_to_string(&file).unwrap(), "{}");
    fs::remove_file(alias).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    assert_eq!(grant(&file, &keys()), PreTrustOutcome::Granted);
    assert_eq!(
        fs::metadata(file).unwrap().permissions().mode() & 0o777,
        0o640
    );
    no_temps(dir.path());
}

#[cfg(windows)]
#[test]
fn windows_open_without_share_delete_fails_once_without_changing_record() {
    use std::os::windows::fs::OpenOptionsExt;
    let (dir, file) = record("{}");
    let held = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&file)
        .unwrap();
    let mut attempts = 0;
    let outcome = grant_with(&file, &keys(), |_, _| attempts += 1);
    assert!(matches!(outcome, PreTrustOutcome::Failed(_)));
    assert_eq!(attempts, 1);
    assert_eq!(fs::read_to_string(file).unwrap(), "{}");
    drop(held);
    no_temps(dir.path());
}
