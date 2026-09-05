use super::*;
use std::path::PathBuf;
use std::sync::Mutex;

struct FakeUserPath {
    value: Mutex<Option<String>>,
    writes: Mutex<Vec<String>>,
}

impl FakeUserPath {
    fn with(value: &str) -> Self {
        Self {
            value: Mutex::new(Some(value.to_string())),
            writes: Mutex::new(Vec::new()),
        }
    }

    fn without_registry() -> Self {
        Self {
            value: Mutex::new(None),
            writes: Mutex::new(Vec::new()),
        }
    }

    fn writes(&self) -> Vec<String> {
        self.writes.lock().unwrap().clone()
    }

    fn value(&self) -> Option<String> {
        self.value.lock().unwrap().clone()
    }
}

impl UserPathStore for FakeUserPath {
    fn read(&self) -> Option<String> {
        self.value()
    }

    fn write(&self, value: &str) -> std::io::Result<()> {
        *self.value.lock().unwrap() = Some(value.to_string());
        self.writes.lock().unwrap().push(value.to_string());
        Ok(())
    }
}

fn temp_root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Users\u\AppData\Local\Temp")
    } else {
        PathBuf::from("/tmp")
    }
}

fn real_bin() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Users\u\.codeterm\bin")
    } else {
        PathBuf::from("/home/u/.codeterm/bin")
    }
}

fn system_bin() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Windows\system32")
    } else {
        PathBuf::from("/usr/bin")
    }
}

/// The shape 427 test runs left behind: a fresh tempdir home per run.
fn dead(tag: &str) -> PathBuf {
    temp_root()
        .join(format!(".tmp{tag}"))
        .join(".codeterm")
        .join("bin")
}

fn text(path: &Path) -> String {
    entry_text(path)
}

fn joined(parts: &[PathBuf]) -> String {
    parts
        .iter()
        .map(|part| text(part))
        .collect::<Vec<_>>()
        .join(";")
}

/// Both halves. A writer that refuses everything passes the refusal alone, and
/// one that accepts everything passes the acceptance alone.
#[test]
fn production_refuses_an_ephemeral_directory_and_still_accepts_a_real_one() {
    let temp = temp_root();
    let current = text(&system_bin());

    assert_eq!(
        user_path_after_adding(&current, &dead("a1b2"), &temp),
        UserPathDecision::RefusedEphemeral,
        "a per-run tempdir bin can never be a durable install"
    );
    assert_eq!(
        user_path_after_adding(&current, &temp, &temp),
        UserPathDecision::RefusedEphemeral,
        "the temp directory itself is refused too"
    );
    assert_eq!(
        user_path_after_adding(&current, &real_bin(), &temp),
        UserPathDecision::Append(format!("{current};{}", text(&real_bin()))),
        "a real install directory must still be appended"
    );
}

/// Regression guard for behaviour that already existed before this change:
/// a directory already on the PATH is never appended a second time.
#[test]
fn containment_guard_refuses_a_directory_already_on_the_path() {
    let temp = temp_root();
    let current = joined(&[system_bin(), real_bin()]);

    assert_eq!(
        user_path_after_adding(&current, &real_bin(), &temp),
        UserPathDecision::AlreadyPresent
    );
    assert_eq!(
        user_path_after_adding(&current.to_uppercase(), &real_bin(), &temp),
        UserPathDecision::AlreadyPresent,
        "the user PATH is case-insensitive"
    );
    assert_eq!(
        user_path_after_adding(&format!("{current};"), &real_bin(), &temp),
        UserPathDecision::AlreadyPresent,
        "a trailing separator is not a new entry"
    );
}

/// The old guard was a substring match over the whole `reg query` output, so a
/// longer entry falsely satisfied it and the real directory was never added.
#[test]
fn containment_guard_does_not_match_a_longer_entry() {
    let temp = temp_root();
    let mut longer = real_bin().into_os_string();
    longer.push("2");
    let current = joined(&[system_bin(), PathBuf::from(longer)]);

    assert!(
        matches!(
            user_path_after_adding(&current, &real_bin(), &temp),
            UserPathDecision::Append(_)
        ),
        "an entry that merely contains ours must not block it"
    );
}

#[test]
fn repair_removes_only_the_dead_temp_shape() {
    let temp = temp_root();
    let survivors = [
        system_bin(),
        real_bin(),
        // Under temp, wrong leaf.
        temp.join(".tmpzz").join("other").join("bin"),
        // Under temp, right leaf, but not a tempdir name.
        temp.join("keep").join(".codeterm").join("bin"),
        // Under temp, right prefix, but one component short.
        temp.join(".tmpyy").join(".codeterm"),
        // Our exact shape, but not under temp.
        PathBuf::from(if cfg!(windows) {
            r"C:\tools\.tmpq\.codeterm\bin"
        } else {
            "/tools/.tmpq/.codeterm/bin"
        }),
    ];
    let current = format!(
        "{};{};{}",
        joined(&survivors[..2]),
        joined(&[dead("aaaa"), dead("bbbb")]),
        joined(&survivors[2..])
    );

    let repair = plan_user_path_repair(&current, &temp);

    assert_eq!(
        repair.removed,
        vec![text(&dead("aaaa")), text(&dead("bbbb"))],
        "only the dead shape may be removed"
    );
    assert_eq!(
        repair.value,
        joined(&survivors),
        "every other entry survives, in order"
    );
}

#[test]
fn repair_reports_what_it_removed_and_is_safe_to_run_twice() {
    let temp = temp_root();
    let clean = joined(&[system_bin(), real_bin()]);
    let store = FakeUserPath::with(&format!(
        "{};{};{}",
        text(&system_bin()),
        joined(&[dead("1111"), dead("2222"), dead("3333")]),
        text(&real_bin())
    ));

    let first = repair_user_path_with(&store, &temp).unwrap();
    assert_eq!(first.len(), 3, "the repair reports how many it removed");
    assert_eq!(store.value().as_deref(), Some(clean.as_str()));

    let second = repair_user_path_with(&store, &temp).unwrap();
    assert!(second.is_empty(), "a second run removes nothing");
    assert_eq!(
        store.writes().len(),
        1,
        "a second run must not write the registry again"
    );
    assert_eq!(store.value().as_deref(), Some(clean.as_str()));
}

/// Companion to the removal test: on a platform with no registry the repair
/// succeeds reporting zero instead of erroring, and never attempts a write.
#[test]
fn repair_is_a_no_op_without_a_registry() {
    let store = FakeUserPath::without_registry();

    let removed = repair_user_path_with(&store, &temp_root()).expect("no registry is not an error");

    assert!(removed.is_empty());
    assert!(store.writes().is_empty());
}

#[test]
fn repair_report_states_the_count_and_the_entries() {
    let removed = vec![text(&dead("aaaa")), text(&dead("bbbb"))];

    let report = user_path_repair_report(&removed);

    assert!(report.starts_with("CodeTerm user PATH repair: removed 2 dead entries"));
    for entry in &removed {
        assert!(report.contains(entry), "report must name {entry}");
    }
    assert!(user_path_repair_report(&[]).contains("removed 0 dead entries"));
}
