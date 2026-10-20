//! storage_migration_ capture: every original is copied byte-exactly with its hash into a
//! sealed backup, the source is never written, and a moving or oversized source refuses.

use super::*;

const BAK_1: &str = "state-backups/crons.json/0000000000001-000001.bak";
const BAK_2: &str = "state-backups/crons.json/0000000000002-000002.bak";
const QUARANTINED: &str = "crons.json.corrupt-0000000000003";
const INTERRUPTED: &str = "crons.json.tmp.42.7";
const SENTINEL_BAK: &str = "state-backups/sentinels.json/0000000000004-000001.bak";

/// Makes `link` a directory link to `target` without privileges: a symlink on Unix, a
/// junction on Windows.
fn link_directory(link: &Path, target: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("directory symlink");
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("cmd");
        crate::process_util::no_window_std(&mut command);
        let created = command
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("run mklink");
        assert!(
            created.status.success(),
            "junction: {}",
            String::from_utf8_lossy(&created.stderr)
        );
    }
}

#[test]
fn storage_migration_capture_backs_up_every_original_with_hash_and_size() {
    let f = Fixture::seeded();
    f.put(BAK_1, CRONS.as_bytes());
    f.put(BAK_2, EMPTY_CRONS.as_bytes());
    f.put(QUARANTINED, b"{broken");
    f.put(INTERRUPTED, b"{\"version\":");
    f.put(SENTINEL_BAK, b"[]");
    let originals = tree(&f.source());
    let original_stamps = stamps(&f.source());

    let capture = f.capture();

    let inventory: Vec<(ImportDomain, SourceRole, &str)> = capture
        .files()
        .iter()
        .map(|file| (file.domain, file.role, file.relative.as_str()))
        .collect();
    assert_eq!(
        inventory,
        [
            (ImportDomain::Cron, SourceRole::Current, "crons.json"),
            (ImportDomain::Cron, SourceRole::RotatingBackup, BAK_1),
            (ImportDomain::Cron, SourceRole::RotatingBackup, BAK_2),
            (ImportDomain::Cron, SourceRole::Quarantined, QUARANTINED),
            (
                ImportDomain::Cron,
                SourceRole::InterruptedWrite,
                INTERRUPTED
            ),
            (
                ImportDomain::Sentinel,
                SourceRole::Current,
                "sentinels.json"
            ),
            (
                ImportDomain::Sentinel,
                SourceRole::RotatingBackup,
                SENTINEL_BAK
            ),
        ]
    );
    for file in capture.files() {
        let original = &originals[&file.relative];
        let Captured::Bytes { size, sha256 } = &file.captured else {
            panic!("{} was not captured", file.relative);
        };
        assert_eq!(*size, original.len() as u64, "{}", file.relative);
        assert_eq!(
            sha256.to_hex(),
            reference_sha256(original),
            "{}",
            file.relative
        );
        let copy = capture.original_path(file);
        assert_eq!(&fs::read(&copy).expect("backup copy"), original);
        let sealed = fs::metadata(&copy).expect("copy metadata").permissions();
        assert!(sealed.readonly(), "{} is sealed", file.relative);
    }
    assert_eq!(tree(&f.source()), originals, "originals keep their bytes");
    assert_eq!(
        stamps(&f.source()),
        original_stamps,
        "originals are never rewritten"
    );

    let manifest = fs::read(f.backup().join("capture.manifest")).expect("capture manifest");
    assert_eq!(Sha256Digest::of(&manifest), capture.digest());
    let parsed = lines(&manifest);
    let format = IMPORT_MANIFEST_FORMAT.to_string();
    assert_eq!(parsed[0], line(&["codeterm-legacy-capture", &format]));
    let files = parsed.iter().filter(|words| words[0] == "file").count();
    assert_eq!(files, capture.files().len());
    assert_eq!(parsed.last(), Some(&line(&["end", "7"])));
}

#[test]
fn storage_migration_sha256_matches_reference_digests() {
    let vectors = [
        (
            &b""[..],
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            &b"abc"[..],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            &b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"[..],
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
    ];
    for (bytes, expected) in vectors {
        assert_eq!(Sha256Digest::of(bytes).to_hex(), expected);
    }
    for length in [1, 55, 56, 57, 63, 64, 65, 119, 120, 127, 128, 1000, 70_000] {
        let bytes: Vec<u8> = (0..length).map(|index| (index * 31 % 251) as u8).collect();
        let digest = Sha256Digest::of(&bytes);
        assert_eq!(digest.to_hex(), reference_sha256(&bytes), "length {length}");
        assert_eq!(Sha256Digest::parse_hex(&digest.to_hex()), Some(digest));
    }
    assert_eq!(Sha256Digest::parse_hex("ABC"), None);

    let f = Fixture::new();
    let large: Vec<u8> = (0..200_003_usize)
        .map(|index| (index % 253) as u8)
        .collect();
    f.put("crons.json", &large);
    let capture = f.capture();
    let Captured::Bytes { size, sha256 } = &capture.files()[0].captured else {
        panic!("captured");
    };
    assert_eq!(*size, large.len() as u64);
    assert_eq!(
        sha256.to_hex(),
        reference_sha256(&large),
        "streamed across chunks"
    );
}

#[test]
fn storage_migration_capture_copies_no_credentials_or_provider_homes() {
    let f = Fixture::seeded();
    for decoy in [
        "credentials.json",
        "secrets/token",
        ".claude/settings.json",
        ".codex/auth.json",
        "mem/knowledge.db",
        "plans.db",
        "crons/stale-mirror/r1.log",
        "state-backups/settings.json/0000000000001-000001.bak",
        "crons.jsonx",
    ] {
        f.put(decoy, b"opaque-decoy");
    }

    let capture = f.capture();

    let captured: Vec<&str> = capture
        .files()
        .iter()
        .map(|file| file.relative.as_str())
        .collect();
    assert_eq!(captured, ["crons.json", "sentinels.json"]);
    let backup: Vec<String> = tree(&f.backup()).into_keys().collect();
    assert_eq!(
        backup,
        [
            "capture.manifest",
            "originals/crons.json",
            "originals/sentinels.json"
        ]
    );
}

#[test]
fn storage_migration_source_change_mid_capture_refuses() {
    let f = Fixture::seeded();
    let current = f.source().join("crons.json");

    let error = SourceCapture::capture_observed(
        &f.source(),
        &f.backup(),
        CaptureLimits::default(),
        &mut |relative: &str| {
            if relative == "crons.json" {
                fs::write(&current, EMPTY_CRONS).expect("concurrent writer");
            }
            CaptureStep::Continue
        },
    )
    .expect_err("a source that moved under the capture refuses");

    assert_eq!(
        refusal(error),
        ImportRefusal::SourceChanged {
            relative: "crons.json".to_string()
        }
    );
    assert!(!f.backup().join("capture.manifest").exists());
    let restart = capture_sources(&f.source(), &f.backup(), CaptureLimits::default())
        .expect_err("the partial staging no longer matches the source");
    assert_eq!(
        refusal(restart),
        ImportRefusal::StagingConflict {
            relative: "crons.json".to_string()
        }
    );
    let fresh = capture_sources(&f.source(), &f.other("backup-2"), CaptureLimits::default())
        .expect("a fresh backup of the settled copy");
    let Captured::Bytes { sha256, .. } = &fresh.files()[0].captured else {
        panic!("captured");
    };
    assert_eq!(sha256.to_hex(), reference_sha256(EMPTY_CRONS.as_bytes()));
}

#[test]
fn storage_migration_listing_change_mid_capture_refuses() {
    let f = Fixture::seeded();
    let added = "state-backups/crons.json/0000000000009-000001.bak";

    let error = SourceCapture::capture_observed(
        &f.source(),
        &f.backup(),
        CaptureLimits::default(),
        &mut |relative: &str| {
            if relative == "sentinels.json" {
                f.put(added, CRONS.as_bytes());
            }
            CaptureStep::Continue
        },
    )
    .expect_err("a new original appearing mid-capture refuses");

    assert_eq!(
        refusal(error),
        ImportRefusal::SourceChanged {
            relative: added.to_string()
        }
    );
    assert!(!f.backup().join("capture.manifest").exists());
}

#[test]
fn storage_migration_unreadable_entry_change_mid_capture_refuses() {
    let f = Fixture::new();
    let current = f.source().join("crons.json");
    fs::create_dir_all(&current).expect("a directory where the file belongs");
    f.put("sentinels.json", SENTINELS.as_bytes());

    let error = SourceCapture::capture_observed(
        &f.source(),
        &f.backup(),
        CaptureLimits::default(),
        &mut |relative: &str| {
            if relative == "crons.json" {
                fs::remove_dir(&current).expect("replace the unreadable entry");
                fs::write(&current, CRONS).expect("with a readable file");
            }
            CaptureStep::Continue
        },
    )
    .expect_err("an unreadable entry that changed under the capture refuses");

    assert_eq!(
        refusal(error),
        ImportRefusal::SourceChanged {
            relative: "crons.json".to_string()
        }
    );
    assert!(!f.backup().join("capture.manifest").exists());
}

#[test]
fn storage_migration_unreadable_entry_kind_change_mid_capture_refuses() {
    let f = Fixture::new();
    let current = f.source().join("crons.json");
    fs::create_dir_all(&current).expect("a directory where the file belongs");
    let elsewhere = f.other("elsewhere");
    fs::create_dir_all(&elsewhere).expect("link target");
    f.put("sentinels.json", SENTINELS.as_bytes());

    let error = SourceCapture::capture_observed(
        &f.source(),
        &f.backup(),
        CaptureLimits::default(),
        &mut |relative: &str| {
            if relative == "crons.json" {
                fs::remove_dir(&current).expect("drop the directory");
                link_directory(&current, &elsewhere);
            }
            CaptureStep::Continue
        },
    )
    .expect_err("a directory that became a link, still unreadable, refuses");

    assert_eq!(
        refusal(error),
        ImportRefusal::SourceChanged {
            relative: "crons.json".to_string()
        }
    );
    assert!(!f.backup().join("capture.manifest").exists());
}

#[test]
fn storage_migration_interrupted_capture_restarts_idempotently() {
    let f = Fixture::seeded();
    f.put(BAK_1, CRONS.as_bytes());
    let originals = tree(&f.source());
    let mut staged = 0;

    let error = SourceCapture::capture_observed(
        &f.source(),
        &f.backup(),
        CaptureLimits::default(),
        &mut |_: &str| {
            staged += 1;
            if staged == 2 {
                CaptureStep::Interrupt
            } else {
                CaptureStep::Continue
            }
        },
    )
    .expect_err("interrupted like a crash");

    assert!(
        matches!(
            error,
            MigrationError::Storage(StorageError::Injected { point: "capture" })
        ),
        "{error:?}"
    );
    assert!(!f.backup().join("capture.manifest").exists());
    let partial = stamps(&f.backup().join("originals"));
    assert_eq!(partial.len(), 2, "two originals were staged");

    let capture = f.capture();

    assert_eq!(capture.files().len(), 3);
    let resumed = stamps(&f.backup().join("originals"));
    for (relative, stamp) in &partial {
        assert_eq!(
            resumed[relative], *stamp,
            "{relative} was reused, not rewritten"
        );
    }
    assert_eq!(
        f.capture().digest(),
        capture.digest(),
        "a repeated capture is idempotent"
    );
    assert_eq!(tree(&f.source()), originals);
}

#[test]
fn storage_migration_capture_over_limit_refuses_without_truncation() {
    let f = Fixture::seeded();
    let originals = tree(&f.source());
    let cron_bytes = CRONS.len() as u64;

    let file_bound = CaptureLimits {
        max_file_bytes: cron_bytes - 1,
        ..CaptureLimits::default()
    };
    let error = capture_sources(&f.source(), &f.backup(), file_bound).expect_err("file bound");
    assert_eq!(
        refusal(error),
        ImportRefusal::OverLimit {
            what: "source file bytes",
            limit: cron_bytes - 1,
            observed: cron_bytes
        }
    );
    assert!(!f.backup().join("originals").join("crons.json").exists());

    let count_bound = CaptureLimits {
        max_files: 1,
        ..CaptureLimits::default()
    };
    let error = capture_sources(&f.source(), &f.other("backup-count"), count_bound)
        .expect_err("file count bound");
    assert_eq!(
        refusal(error),
        ImportRefusal::OverLimit {
            what: "source files",
            limit: 1,
            observed: 2
        }
    );

    let total_bound = CaptureLimits {
        max_total_bytes: cron_bytes + 1,
        ..CaptureLimits::default()
    };
    let error = capture_sources(&f.source(), &f.other("backup-total"), total_bound)
        .expect_err("capture total bound");
    assert_eq!(
        refusal(error),
        ImportRefusal::OverLimit {
            what: "capture bytes",
            limit: cron_bytes + 1,
            observed: cron_bytes + SENTINELS.len() as u64
        }
    );
    assert!(!f.other("backup-total").join("capture.manifest").exists());
    assert!(!f
        .other("backup-total")
        .join("originals/sentinels.json")
        .exists());
    assert_eq!(tree(&f.source()), originals, "originals stay whole");
}
