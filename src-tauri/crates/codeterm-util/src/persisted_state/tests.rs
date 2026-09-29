use super::*;

fn parse_state(text: &str) -> Result<u32, String> {
    text.strip_prefix("v=")
        .ok_or_else(|| "missing v=".to_string())?
        .parse::<u32>()
        .map_err(|error| error.to_string())
}

fn validate_state(text: &str) -> Result<(), String> {
    parse_state(text).map(|_| ())
}

fn fixture(name: &str) -> (tempfile::TempDir, StateFile) {
    let dir = tempfile::tempdir().unwrap();
    let file = StateFile::new(dir.path().join(name)).with_validator(validate_state);
    (dir, file)
}

#[test]
fn persisted_state_loader_rotates_backups() {
    let (_dir, file) = fixture("rotate.state");
    for n in 0..DEFAULT_BACKUPS + 3 {
        file.save(&format!("v={n}")).unwrap();
    }
    let backups = file.backups();
    assert_eq!(backups.len(), DEFAULT_BACKUPS);
    let newest = std::fs::read_to_string(backups.last().unwrap()).unwrap();
    assert_eq!(newest, format!("v={}", DEFAULT_BACKUPS + 2));
    let oldest = std::fs::read_to_string(&backups[0]).unwrap();
    assert_eq!(oldest, "v=3");

    file.save(&newest).unwrap();
    assert_eq!(
        file.backups().len(),
        DEFAULT_BACKUPS,
        "an identical save adds no backup"
    );
}

#[test]
fn without_backups_writes_no_backup_dir() {
    let (_dir, file) = fixture("plain.state");
    let file = file.without_backups();
    file.save("v=0").unwrap();
    assert!(file.backups().is_empty());
    assert!(!file.backup_dir().exists());
}

#[test]
fn corrupt_file_is_preserved_byte_identical_and_restored_from_newest_backup() {
    let (_dir, file) = fixture("state.state");
    file.save("v=1").unwrap();
    file.save("v=2").unwrap();
    let broken = b"v=3tr".to_vec();
    std::fs::write(file.path(), &broken).unwrap();

    let loaded = file.load(parse_state);
    let Loaded::Recovered { value, backup } = loaded else {
        panic!("expected recovery, got {loaded:?}");
    };
    assert_eq!(value, 2);
    assert_eq!(backup, *file.backups().last().unwrap());

    let copies = file.corrupt_copies();
    assert_eq!(copies.len(), 1);
    assert_eq!(std::fs::read(&copies[0]).unwrap(), broken);
    assert_eq!(std::fs::read_to_string(file.path()).unwrap(), "v=2");
    assert!(file.held().is_none());

    let incidents = incidents_for(file.path());
    assert_eq!(incidents.len(), 1, "one incident per corrupt file");
    assert_eq!(incidents[0].preserved.as_ref(), Some(&copies[0]));
    assert_eq!(incidents[0].recovered_from.as_ref(), Some(&backup));
    assert!(!incidents[0].held);
}

#[test]
fn invalid_newest_backup_falls_back_to_an_older_valid_one() {
    let (_dir, file) = fixture("fallback.state");
    file.save("v=1").unwrap();
    file.save("v=2").unwrap();
    std::fs::write(file.backups().last().unwrap(), "not valid").unwrap();
    std::fs::write(file.path(), "also not valid").unwrap();

    match file.load(parse_state) {
        Loaded::Recovered { value, .. } => assert_eq!(value, 1),
        other => panic!("expected recovery, got {other:?}"),
    }
}

#[test]
fn corrupt_file_without_backup_holds_every_write_until_it_loads_again() {
    let (_dir, file) = fixture("held.state");
    let broken = b"v=".to_vec();
    std::fs::write(file.path(), &broken).unwrap();

    match file.load(parse_state) {
        Loaded::Unrecoverable { raw } => {
            assert_eq!(raw.as_deref().map(str::as_bytes), Some(broken.as_slice()))
        }
        other => panic!("expected unrecoverable, got {other:?}"),
    }
    assert!(
        !file.path().exists(),
        "defaults are never written in its place"
    );
    let copies = file.corrupt_copies();
    assert_eq!(copies.len(), 1);
    assert_eq!(std::fs::read(&copies[0]).unwrap(), broken);

    let hold = file.held().expect("path is held");
    assert_eq!(hold.preserved.as_ref(), Some(&copies[0]));
    assert!(file.save("v=0").is_err());
    assert!(!file.path().exists());

    std::fs::write(file.path(), "v=7").unwrap();
    assert!(matches!(file.load(parse_state), Loaded::Parsed(_)));
    assert!(file.held().is_none(), "a clean load releases the hold");
    file.save("v=8").unwrap();
    assert_eq!(
        std::fs::read(&copies[0]).unwrap(),
        broken,
        "the kept copy is untouched"
    );
}

#[test]
fn save_preserves_an_existing_file_that_fails_validation() {
    let (_dir, file) = fixture("edited.state");
    file.save("v=1").unwrap();
    let hand_edit = b"v=oops".to_vec();
    std::fs::write(file.path(), &hand_edit).unwrap();

    file.save("v=2").unwrap();

    let copies = file.corrupt_copies();
    assert_eq!(copies.len(), 1);
    assert_eq!(std::fs::read(&copies[0]).unwrap(), hand_edit);
    assert_eq!(std::fs::read_to_string(file.path()).unwrap(), "v=2");
}

#[test]
fn missing_file_is_missing_not_corrupt() {
    let (_dir, file) = fixture("absent.state");
    assert!(matches!(file.load(parse_state), Loaded::Missing));
    assert!(file.corrupt_copies().is_empty());
    assert!(incidents_for(file.path()).is_empty());
}

#[test]
fn corrupt_copies_keep_only_the_newest() {
    let (_dir, file) = fixture("many.state");
    for n in 0..MAX_CORRUPT_COPIES + 2 {
        std::fs::write(file.path(), format!("broken {n}")).unwrap();
        let _ = file.load(parse_state);
    }
    let copies = file.corrupt_copies();
    assert_eq!(copies.len(), MAX_CORRUPT_COPIES);
    let newest = std::fs::read_to_string(copies.last().unwrap()).unwrap();
    assert_eq!(newest, format!("broken {}", MAX_CORRUPT_COPIES + 1));
}
