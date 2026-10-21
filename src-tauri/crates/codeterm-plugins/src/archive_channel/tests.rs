use super::*;

fn archive(entries: &[(&[u8], tar::EntryType, &[u8])]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for (name, kind, data) in entries {
        let mut header = tar::Header::new_old();
        header.as_old_mut().name[..name.len()].copy_from_slice(name);
        header.set_entry_type(*kind);
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, *data).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

fn channel_archive() -> Vec<u8> {
    archive(&[
        (
            b"channel.json",
            tar::EntryType::Regular,
            br#"{"name":"codeterm-core","plugins":[]}"#,
        ),
        (b"provider-demo/", tar::EntryType::Directory, b""),
        (
            b"provider-demo/plugin.json",
            tar::EntryType::Regular,
            br#"{"id":"demo"}"#,
        ),
    ])
}

fn checksum(bytes: &[u8]) -> Vec<u8> {
    format!("{:x}  codeterm-core.tar.gz\n", Sha256::digest(bytes)).into_bytes()
}

#[test]
fn archive_urls_are_remote_tarballs_only() {
    assert!(is_archive_url(
        "https://github.com/rollacode/codeterm-releases/releases/download/codeterm-core-latest/codeterm-core.tar.gz"
    ));
    assert!(!is_archive_url(
        "https://github.com/rollacode/codeterm.git#master:plugins"
    ));
    assert!(!is_archive_url("file:///tmp/codeterm-core.tar.gz"));
}

#[test]
fn verified_archive_unpacks_the_channel_tree() {
    let bytes = channel_archive();
    let sum = checksum(&bytes);
    let dest = tempfile::tempdir().unwrap();
    let target = dest.path().join("channel");
    materialize(
        |url, _| {
            Ok(if url.ends_with(".sha256") {
                sum.clone()
            } else {
                bytes.clone()
            })
        },
        "https://example.invalid/codeterm-core.tar.gz",
        &target,
    )
    .unwrap();
    assert!(target.join("channel.json").is_file());
    assert_eq!(
        std::fs::read_to_string(target.join("provider-demo/plugin.json")).unwrap(),
        r#"{"id":"demo"}"#
    );
}

#[test]
fn checksum_mismatch_is_an_integrity_failure_and_writes_nothing() {
    let bytes = channel_archive();
    let dest = tempfile::tempdir().unwrap();
    let target = dest.path().join("channel");
    let error = materialize(
        |url, _| {
            Ok(if url.ends_with(".sha256") {
                checksum(b"other bytes")
            } else {
                bytes.clone()
            })
        },
        "https://example.invalid/codeterm-core.tar.gz",
        &target,
    )
    .unwrap_err();
    assert_eq!(error, INTEGRITY_FAILED);
    assert!(!target.exists());
    assert!(parse_checksum(b"not-a-sum  file").is_none());
}

#[test]
fn fetch_failures_propagate_their_cause() {
    let dest = tempfile::tempdir().unwrap();
    let error = materialize(
        |_, _| Err("release asset not found".into()),
        "https://example.invalid/codeterm-core.tar.gz",
        dest.path(),
    )
    .unwrap_err();
    assert_eq!(error, "release asset not found");
}

#[test]
fn escaping_and_link_entries_are_refused() {
    let dest = tempfile::tempdir().unwrap();
    let escaping = archive(&[(b"../evil.txt", tar::EntryType::Regular, b"x")]);
    assert!(unpack(&escaping, dest.path())
        .unwrap_err()
        .contains("escapes its root"));
    assert!(!dest.path().parent().unwrap().join("evil.txt").exists());
    let link = archive(&[(b"link", tar::EntryType::Symlink, b"")]);
    assert!(unpack(&link, dest.path())
        .unwrap_err()
        .contains("not allowed"));
}
