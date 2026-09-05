use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

struct Paths {
    _root: tempfile::TempDir,
    dmp: PathBuf,
    txt: PathBuf,
}

fn paths() -> Paths {
    let root = tempfile::tempdir().expect("temp dir");
    let dmp = root.path().join("crash-native.dmp");
    let txt = root.path().join("crash-native.txt");
    Paths {
        _root: root,
        dmp,
        txt,
    }
}

fn compose(outcome: &DumpOutcome, dmp: &Path) -> String {
    format!("CodeTerm crash report\n{}\n", outcome.render(dmp))
}

#[test]
fn an_induced_abort_leaves_a_record_naming_the_written_dump() {
    let p = paths();
    let outcome = record_guarded_dump(
        &p.dmp,
        &p.txt,
        |o| compose(o, &p.dmp),
        || {
            std::fs::write(&p.dmp, b"minidump bytes").expect("dump write");
            true
        },
    );

    assert_eq!(outcome, DumpOutcome::Written { bytes: 14 });
    let record = std::fs::read_to_string(&p.txt).expect("a record exists after the abort path");
    assert!(record.contains(WRITTEN), "record was: {record}");
    assert!(record.contains("14 bytes"), "record was: {record}");
    assert!(!record.contains(ATTEMPTED), "record was: {record}");
}

#[test]
fn a_dump_write_that_fails_still_leaves_a_record_saying_so() {
    let p = paths();
    let outcome = record_guarded_dump(&p.dmp, &p.txt, |o| compose(o, &p.dmp), || false);

    assert_eq!(outcome, DumpOutcome::Failed(REASON_RETURNED_FALSE));
    let record = std::fs::read_to_string(&p.txt).expect("a record exists after a failed dump");
    assert!(record.contains(FAILED), "record was: {record}");
    assert!(record.contains(REASON_RETURNED_FALSE), "record was: {record}");
}

#[test]
fn a_dump_write_that_dies_leaves_the_attempted_record_behind() {
    // The record must already be on disk while the dump write runs — asserted from
    // inside it, the only place the 2026-09-05 "died mid-write" state is observable.
    let p = paths();
    let observed_during_write = AtomicBool::new(false);

    let outcome = record_guarded_dump(
        &p.dmp,
        &p.txt,
        |o| compose(o, &p.dmp),
        || {
            let mid_write = std::fs::read_to_string(&p.txt)
                .expect("the record is on disk before the dump write runs");
            observed_during_write.store(mid_write.contains(ATTEMPTED), Ordering::Release);
            panic!("write_minidump died");
        },
    );

    assert!(
        observed_during_write.load(Ordering::Acquire),
        "a process dying inside write_minidump must leave a record saying the outcome is unknown"
    );
    assert_eq!(outcome, DumpOutcome::Failed(REASON_PANICKED));
    let record = std::fs::read_to_string(&p.txt).expect("a record exists after a fatal dump write");
    assert!(record.contains(REASON_PANICKED), "record was: {record}");
}

#[test]
fn a_zero_byte_dump_is_recorded_as_failed_not_written() {
    // The second death's artifact: a 0-byte .dmp. A writer that claims success
    // is not believed over the file it left.
    let p = paths();
    let outcome = record_guarded_dump(
        &p.dmp,
        &p.txt,
        |o| compose(o, &p.dmp),
        || {
            std::fs::write(&p.dmp, b"").expect("empty dump");
            true
        },
    );

    assert_eq!(outcome, DumpOutcome::Failed(REASON_EMPTY));
    let record = std::fs::read_to_string(&p.txt).expect("a record exists for the empty dump");
    assert!(record.contains(REASON_EMPTY), "record was: {record}");
    assert!(!record.contains(WRITTEN), "record was: {record}");
}

#[test]
fn the_record_distinguishes_written_failed_and_outcome_unknown() {
    let dmp = Path::new("crash-native.dmp");
    let attempted = DumpOutcome::Attempted.render(dmp);
    let written = DumpOutcome::Written { bytes: 1_094_014 }.render(dmp);
    let failed = DumpOutcome::Failed(REASON_EMPTY).render(dmp);

    assert!(attempted.contains(ATTEMPTED));
    assert!(written.contains(WRITTEN) && !written.contains(ATTEMPTED));
    assert!(failed.contains(FAILED) && !failed.contains(WRITTEN));
    assert_ne!(attempted, written);
    assert_ne!(written, failed);
    assert_ne!(attempted, failed);
}

#[test]
fn a_dump_the_writer_claims_but_never_created_is_recorded_as_failed() {
    let p = paths();
    let outcome = record_guarded_dump(&p.dmp, &p.txt, |o| compose(o, &p.dmp), || true);

    assert_eq!(outcome, DumpOutcome::Failed(REASON_MISSING));
    assert!(std::fs::read_to_string(&p.txt)
        .expect("record")
        .contains(REASON_MISSING));
}
