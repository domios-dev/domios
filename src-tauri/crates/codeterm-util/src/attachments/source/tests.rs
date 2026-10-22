use super::*;

#[test]
fn empty_and_missing_sources_never_create_an_attachment() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.mov");
    let target = root.path().join("attachment.mov");
    assert!(copy_complete_file(&source, &target, 100).is_err());
    assert!(!target.exists());
    std::fs::write(&source, []).unwrap();
    assert_eq!(
        copy_complete_file(&source, &target, 100),
        Err("Empty file".into())
    );
    assert!(!target.exists());
}

#[test]
fn oversized_source_is_refused_without_creating_a_destination() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.mov");
    let target = root.path().join("attachment.mov");
    std::fs::write(&source, b"recording").unwrap();
    assert!(copy_complete_file(&source, &target, 2).is_err());
    assert!(!target.exists());
}

#[test]
fn growing_source_is_refused_without_timing_or_polling() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("recording.mov");
    std::fs::write(&source, b"first").unwrap();
    let mut input = Source::open(&source, 100).unwrap();
    struct GrowingOutput<'a> {
        source: &'a Path,
        bytes: Vec<u8>,
    }
    impl Write for GrowingOutput<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            OpenOptions::new()
                .append(true)
                .open(self.source)?
                .write_all(b"next")?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = GrowingOutput {
        source: &source,
        bytes: Vec::new(),
    };
    #[cfg(windows)]
    {
        assert!(input.copy_to(&source, &mut output).is_err());
    }
    #[cfg(not(windows))]
    {
        assert_eq!(input.copy_to(&source, &mut output), Err(WRITING.into()));
        assert_eq!(std::fs::read(&source).unwrap(), b"firstnext");
    }
}

#[test]
fn source_changes_are_not_reused_across_drops() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("recording.mov");
    std::fs::write(&source, b"first").unwrap();
    let first = super::super::materialize_dropped_path(&source).unwrap();
    std::fs::write(&source, b"second").unwrap();
    let second = super::super::materialize_dropped_path(&source).unwrap();
    assert_ne!(first, second);
    std::fs::remove_file(source).unwrap();
    assert_eq!(std::fs::read(&first).unwrap(), b"first");
    assert_eq!(std::fs::read(&second).unwrap(), b"second");
    std::fs::remove_file(first).unwrap();
    std::fs::remove_file(second).unwrap();
}

#[test]
fn existing_destination_is_never_followed_or_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    std::fs::write(&source, b"source").unwrap();
    std::fs::write(&target, b"keep").unwrap();
    assert!(copy_complete_file(&source, &target, 100).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn symlink_source_is_copied_to_a_regular_independent_file() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("Screen Recording.mov");
    let source = root.path().join("alias.mov");
    let target = root.path().join("attachment.mov");
    let content = vec![7u8; 4 * 1024 * 1024 + 1];
    std::fs::write(&original, &content).unwrap();
    std::os::unix::fs::symlink(&original, &source).unwrap();
    assert_eq!(
        copy_complete_file(&source, &target, 8 * 1024 * 1024).unwrap(),
        content.len() as u64
    );
    assert!(std::fs::symlink_metadata(&target)
        .unwrap()
        .file_type()
        .is_file());
    std::fs::write(&original, b"changed").unwrap();
    std::fs::remove_file(original).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), content);
}

#[cfg(unix)]
#[test]
fn dangling_symlink_in_the_chat_bucket_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let bucket = super::super::bucket_dir("chat");
    std::fs::create_dir_all(&bucket).unwrap();
    let source = bucket.join(format!("{}.mov", uuid::Uuid::new_v4().simple()));
    std::os::unix::fs::symlink(root.path().join("missing"), &source).unwrap();
    assert!(super::super::materialize_dropped_path(&source).is_err());
    std::fs::remove_file(source).unwrap();
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[test]
fn open_writer_is_refused_even_while_it_is_paused() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("recording.mov");
    let target = root.path().join("attachment.mov");
    let mut writer = File::create(&source).unwrap();
    writer.write_all(b"first").unwrap();
    assert!(copy_complete_file(&source, &target, 100).is_err());
    assert!(!target.exists());
    drop(writer);
    assert_eq!(copy_complete_file(&source, &target, 100).unwrap(), 5);
    assert_eq!(std::fs::read(target).unwrap(), b"first");
}

#[test]
fn read_only_handle_does_not_prevent_copying_a_complete_file() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("ready.mov");
    let target = root.path().join("attachment.mov");
    std::fs::write(&source, b"complete").unwrap();
    let _reader = File::open(&source).unwrap();
    assert_eq!(copy_complete_file(&source, &target, 100).unwrap(), 8);
    assert_eq!(std::fs::read(target).unwrap(), b"complete");
}
