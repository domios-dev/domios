use super::*;
use std::io::Write;

#[test]
fn rejects_traversal_network_device_and_ads_paths() {
    for raw in [
        "relative.md",
        "/tmp/../x.md",
        "/tmp/./x.md",
        "//server/share/x.md",
        r"\\server\share\x.md",
        r"\\?\C:\x.md",
        r"\\.\C:\x.md",
        "/tmp/x.md:stream",
        "/tmp/NUL.txt",
        "/tmp/COM1.md",
        "/tmp/CONIN$.md",
        "/tmp/COM\u{00b9}.md",
    ] {
        assert!(checked_path(raw).is_err(), "{raw}");
    }
}

#[test]
fn reads_are_bounded_even_when_the_open_file_grows() {
    let temp = tempfile::tempdir().unwrap();
    let path = crate::service::local_preview::html_preview::preview_test_root(temp.path()).join("grow.txt");
    std::fs::write(&path, b"small").unwrap();
    let mut source = PreviewSource::open(path.to_str().unwrap(), &|_, _| Ok(())).unwrap();
    File::options()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&[b'x'; 100])
        .unwrap();
    assert!(source.read(8).unwrap_err().contains("exceeds"));
}

#[test]
fn refuses_hardlink_aliases() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::service::local_preview::html_preview::preview_test_root(temp.path());
    let original = root.join("original.txt");
    let alias = root.join("alias.txt");
    std::fs::write(&original, "opaque").unwrap();
    std::fs::hard_link(&original, &alias).unwrap();
    assert!(PreviewSource::open(alias.to_str().unwrap(), &|_, _| Ok(())).is_err());
}

#[test]
fn opened_handle_cannot_be_redirected_by_replacing_the_path() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::service::local_preview::html_preview::preview_test_root(temp.path());
    let path = root.join("read.txt");
    std::fs::write(&path, "original").unwrap();
    let mut source = PreviewSource::open(path.to_str().unwrap(), &|_, _| Ok(())).unwrap();
    #[cfg(unix)]
    {
        std::fs::rename(&path, root.join("old.txt")).unwrap();
        std::fs::write(&path, "replacement").unwrap();
    }
    #[cfg(windows)]
    assert!(std::fs::rename(&path, root.join("old.txt")).is_err());
    assert_eq!(source.read(100).unwrap(), b"original");
}

#[cfg(unix)]
#[test]
fn refuses_symlink_file_and_ancestor_escapes() {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::service::local_preview::html_preview::preview_test_root(temp.path());
    std::fs::create_dir(root.join("target")).unwrap();
    std::fs::write(root.join("target/read.txt"), "opaque").unwrap();
    std::os::unix::fs::symlink(root.join("target"), root.join("alias")).unwrap();
    std::os::unix::fs::symlink(root.join("target/read.txt"), root.join("file.txt")).unwrap();
    for path in [root.join("alias/read.txt"), root.join("file.txt")] {
        assert!(PreviewSource::open(path.to_str().unwrap(), &|_, _| Ok(())).is_err());
    }
}

#[test]
fn guard_refuses_before_canonicalization_or_file_access() {
    let temp = tempfile::tempdir().unwrap();
    let path = crate::service::local_preview::html_preview::preview_test_root(temp.path()).join("missing.txt");
    let error = PreviewSource::open(path.to_str().unwrap(), &|_, canonical| {
        assert!(canonical.is_none());
        Err("reference refusal".into())
    })
    .err()
    .unwrap();
    assert_eq!(error, "reference refusal");
}
