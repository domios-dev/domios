use super::*;

const LIVE_EPOCH: &str = "0000000000000001";
const LIVE_BACKUP: &str = "state-backups/crons.json/0000000000001-000001.bak";
const LIVE_QUARANTINE: &str = "sentinels.json.corrupt-0000000000002";

fn seed_live_root(f: &Fixture) {
    put_live(f, "crons.json", CRONS.as_bytes());
    put_live(f, "sentinels.json", SENTINELS.as_bytes());
    put_live(f, LIVE_BACKUP, EMPTY_CRONS.as_bytes());
    put_live(f, LIVE_QUARANTINE, b"[]");
}

fn put_live(f: &Fixture, relative: &str, bytes: &[u8]) {
    let path = f.instance().join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("live dirs");
    fs::write(path, bytes).expect("live original");
}

fn live_original_hashes(root: &Path) -> BTreeMap<String, String> {
    ["crons.json", "sentinels.json", LIVE_BACKUP, LIVE_QUARANTINE]
        .into_iter()
        .map(|relative| {
            let bytes = fs::read(root.join(relative)).expect("live original bytes");
            (relative.to_string(), Sha256Digest::of(&bytes).to_hex())
        })
        .collect()
}

fn live_archive(root: &Path) -> PathBuf {
    root.join("storage")
        .join("legacy-originals")
        .join(LIVE_EPOCH)
}

#[test]
fn import_live_root_captures_seals_and_imports_without_changing_originals() {
    let f = Fixture::new();
    seed_live_root(&f);
    let before = live_original_hashes(&f.instance());
    let storage = f.open();

    let outcome = storage
        .import_live_root(&LiveRootImportRequest::new(), &LegacyCodecs)
        .expect("live import");

    let receipt = match outcome {
        ImportOutcome::Committed(receipt) => *receipt,
        other => panic!("expected commit, got {other:?}"),
    };
    let archive = live_archive(&f.instance());
    assert_eq!(live_original_hashes(&f.instance()), before);
    assert_eq!(
        fs::read(archive.join("originals/crons.json"))
            .expect("archived crons")
            .as_slice(),
        CRONS.as_bytes()
    );
    assert_eq!(
        fs::read(archive.join("originals").join(LIVE_BACKUP))
            .expect("archived rotating backup")
            .as_slice(),
        EMPTY_CRONS.as_bytes()
    );
    assert!(archive.join("capture.manifest").is_file());
    assert_eq!(
        receipt.capture_sha256,
        Sha256Digest::of(&fs::read(archive.join("capture.manifest")).expect("capture manifest"))
    );
    assert_eq!(receipt.archive, Ok(()));
    assert!(cron_snapshot(&storage)
        .states
        .iter()
        .any(|state| state.job_id.as_str() == "stale-mirror"));
    close(storage);
}

#[test]
fn import_live_root_refuses_a_source_that_changes_during_capture() {
    let f = Fixture::new();
    seed_live_root(&f);
    let storage = f.open();
    let changed = f.instance().join("crons.json");

    let error = storage
        .capture_live_root_observed(CaptureLimits::default(), DEADLINE, &mut |relative| {
            if relative == "crons.json" {
                fs::write(&changed, EMPTY_CRONS).expect("simulate concurrent update");
            }
            CaptureStep::Continue
        })
        .expect_err("moving live source must refuse");

    assert_eq!(
        refusal(error),
        ImportRefusal::SourceChanged {
            relative: "crons.json".to_string()
        }
    );
    assert!(!live_archive(&f.instance())
        .join("capture.manifest")
        .exists());
    close(storage);
}

#[test]
fn import_live_root_restarts_a_partial_capture_idempotently() {
    let f = Fixture::new();
    seed_live_root(&f);
    let before = live_original_hashes(&f.instance());
    let storage = f.open();
    let mut stopped = false;
    let error = storage
        .capture_live_root_observed(CaptureLimits::default(), DEADLINE, &mut |relative| {
            if relative == "crons.json" && !stopped {
                stopped = true;
                CaptureStep::Interrupt
            } else {
                CaptureStep::Continue
            }
        })
        .expect_err("observer interrupts after staging");
    assert!(matches!(error, MigrationError::Storage(_)));
    assert!(!live_archive(&f.instance())
        .join("capture.manifest")
        .exists());

    let outcome = storage
        .import_live_root(&LiveRootImportRequest::new(), &LegacyCodecs)
        .expect("resume partial capture and import");

    assert!(matches!(outcome, ImportOutcome::Committed(_)));
    assert_eq!(live_original_hashes(&f.instance()), before);
    assert_eq!(
        fs::read(live_archive(&f.instance()).join("originals/crons.json"))
            .expect("archived crons")
            .as_slice(),
        CRONS.as_bytes()
    );
    close(storage);
}

#[test]
fn import_live_root_reuses_a_sealed_set_after_live_sources_change() {
    let f = Fixture::new();
    seed_live_root(&f);
    let storage = f.open();
    let capture = storage
        .capture_live_root_observed(CaptureLimits::default(), DEADLINE, &mut |_| {
            CaptureStep::Continue
        })
        .expect("seal live inventory");
    let archive = live_archive(&f.instance());
    let sealed_crons = fs::read(archive.join("originals/crons.json")).expect("sealed crons");
    put_live(&f, "crons.json", EMPTY_CRONS.as_bytes());

    let outcome = storage
        .import_live_root(&LiveRootImportRequest::new(), &LegacyCodecs)
        .expect("import existing seal");

    let ImportOutcome::Committed(receipt) = outcome else {
        panic!("expected commit");
    };
    assert_eq!(receipt.capture_sha256, capture.digest());
    assert_eq!(
        fs::read(archive.join("originals/crons.json")).expect("sealed crons remain"),
        sealed_crons
    );
    assert!(cron_snapshot(&storage)
        .states
        .iter()
        .any(|state| state.job_id.as_str() == "stale-mirror"));
    close(storage);
}
