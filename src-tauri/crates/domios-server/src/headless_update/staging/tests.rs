use super::*;

fn offer() -> VerifiedManifest {
    codeterm_update::headless::verify_feed(
        include_bytes!("../../../../codeterm-update/src/headless/fixtures/valid.json"),
        include_str!("../../../../codeterm-update/src/headless/fixtures/test-public-key.txt")
            .trim(),
        "linux-x86_64",
    )
    .unwrap()
}

#[test]
fn headless_stage_verifies_before_writing_and_retains_current_bundle() {
    let root = tempfile::tempdir().unwrap();
    let active = root.path().join("old-server");
    std::fs::write(&active, b"running old server").unwrap();
    let archive = root.path().join("archive.tar.gz");
    std::fs::write(
        &archive,
        include_bytes!("../../../../codeterm-update/src/headless/fixtures/archive.tar.gz"),
    )
    .unwrap();
    let staged = root.path().join("next");
    std::fs::create_dir(&staged).unwrap();
    assert_eq!(stage(&offer(), &archive, &staged).unwrap(), "a".repeat(40));
    assert_eq!(
        std::fs::read(staged.join(".domios-version")).unwrap(),
        b"9.0.0\n"
    );
    assert!(staged.join("cli/codeterm").is_file());
    assert!(staged.join("dist/index.html").is_file());
    assert_eq!(std::fs::read(&active).unwrap(), b"running old server");
    std::fs::write(&archive, b"wrong download").unwrap();
    let empty = root.path().join("refused");
    std::fs::create_dir(&empty).unwrap();
    assert_eq!(
        stage(&offer(), &archive, &empty).unwrap_err().code,
        ErrorCode::ArchiveMismatch
    );
    assert_eq!(std::fs::read_dir(empty).unwrap().count(), 0);
    assert_eq!(std::fs::read(active).unwrap(), b"running old server");
}

#[test]
fn headless_stage_missing_download_and_occupied_destination_do_not_change_files() {
    let root = tempfile::tempdir().unwrap();
    let offer = offer();
    assert!(stage(&offer, &root.path().join("missing"), root.path()).is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    let occupied = root.path().join("old");
    std::fs::write(&occupied, b"old bytes").unwrap();
    assert_eq!(
        stage(&offer, &root.path().join("missing"), root.path())
            .unwrap_err()
            .code,
        ErrorCode::StageFailed
    );
    assert_eq!(std::fs::read(occupied).unwrap(), b"old bytes");
}

fn archive(path: &[u8], entry_type: tar::EntryType, mode: u32) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_ustar();
    header.as_mut_bytes()[..path.len()].copy_from_slice(path);
    header.set_mode(mode);
    header.set_size(1);
    header.set_entry_type(entry_type);
    header.set_cksum();
    tar.append(&header, b"x".as_slice()).unwrap();
    let bytes = tar.into_inner().unwrap();
    let mut compressed = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    compressed.write_all(&bytes).unwrap();
    compressed.finish().unwrap()
}

#[test]
fn headless_stage_rejects_escaping_links_special_modes_and_incomplete_bundles() {
    for (path, kind, mode) in [
        (b"../escape".as_slice(), tar::EntryType::Regular, 0o644),
        (b"/absolute".as_slice(), tar::EntryType::Regular, 0o644),
        (b"dist/link".as_slice(), tar::EntryType::Symlink, 0o644),
        (b"dist/hard".as_slice(), tar::EntryType::Link, 0o644),
        (b"dist/suid".as_slice(), tar::EntryType::Regular, 0o4755),
        (b"dist/only".as_slice(), tar::EntryType::Regular, 0o644),
    ] {
        let root = tempfile::tempdir().unwrap();
        assert!(unpack(archive(path, kind, mode).as_slice(), root.path()).is_err());
    }
}

#[test]
fn headless_stage_refuses_cli_mismatch_and_component_digest_mismatch() {
    for relative in ["cli/domios", "domios-server/domios-server"] {
        let root = tempfile::tempdir().unwrap();
        unpack(
            include_bytes!("../../../../codeterm-update/src/headless/fixtures/archive.tar.gz")
                .as_slice(),
            root.path(),
        )
        .unwrap();
        std::fs::write(root.path().join(relative), b"changed").unwrap();
        assert!(validate_bundle(root.path()).is_err());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn headless_atomic_bundle_swap_and_rollback_preserve_both_slots() {
    let root = tempfile::tempdir().unwrap();
    let previous = root.path().join("previous");
    let next = root.path().join("next");
    for (path, value) in [(&previous, b"old"), (&next, b"new")] {
        std::fs::create_dir(path).unwrap();
        std::fs::write(path.join("binary"), value).unwrap();
    }
    let active = root.path().join("active");
    std::os::unix::fs::symlink(&previous, &active).unwrap();
    assert!(switch_link(&active, &root.path().join("missing")).is_err());
    assert_eq!(std::fs::read(active.join("binary")).unwrap(), b"old");
    switch_link(&active, &next).unwrap();
    assert_eq!(std::fs::read(active.join("binary")).unwrap(), b"new");
    switch_link(&active, &previous).unwrap();
    assert_eq!(std::fs::read(active.join("binary")).unwrap(), b"old");
    assert_eq!(std::fs::read(next.join("binary")).unwrap(), b"new");
}
