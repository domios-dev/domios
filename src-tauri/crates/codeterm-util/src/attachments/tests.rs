use super::*;

fn unique_bucket() -> String {
    format!("test-{}", uuid::Uuid::new_v4())
}

#[test]
fn test_save_and_read_back() {
    let bucket = unique_bucket();
    let path = save(
        &bucket,
        "hello.txt",
        b"hello world",
        EvictionPolicy::MaxCount(10),
    )
    .unwrap();
    assert!(path.exists());
    let content = std::fs::read(&path).unwrap();
    assert_eq!(content, b"hello world");
    let _ = std::fs::remove_dir_all(bucket_dir(&bucket));
}

#[test]
fn test_save_with_uuid_generates_unique_names() {
    let bucket = unique_bucket();
    let path1 = save_with_uuid(&bucket, "png", b"one", EvictionPolicy::MaxCount(10)).unwrap();
    let path2 = save_with_uuid(&bucket, "png", b"two", EvictionPolicy::MaxCount(10)).unwrap();
    assert_ne!(path1, path2);
    assert!(path1.exists());
    assert!(path2.exists());
    let _ = std::fs::remove_dir_all(bucket_dir(&bucket));
}

#[test]
fn test_save_with_uuid_handles_leading_dot() {
    let bucket = unique_bucket();
    let path = save_with_uuid(&bucket, ".png", b"data", EvictionPolicy::MaxCount(10)).unwrap();
    let name = path.file_name().unwrap().to_string_lossy();
    assert!(!name.contains(".."));
    assert!(name.ends_with(".png"));
    let _ = std::fs::remove_dir_all(bucket_dir(&bucket));
}

#[test]
fn test_save_without_extension() {
    let bucket = unique_bucket();
    let path = save_with_uuid(&bucket, "", b"data", EvictionPolicy::MaxCount(10)).unwrap();
    let name = path.file_name().unwrap().to_string_lossy();
    assert!(!name.contains('.'));
    let _ = std::fs::remove_dir_all(bucket_dir(&bucket));
}

#[test]
fn test_max_count_eviction_keeps_newest() {
    let bucket = unique_bucket();
    for i in 0..5 {
        save(
            &bucket,
            &format!("f{}.txt", i),
            &[i as u8],
            EvictionPolicy::MaxCount(100),
        )
        .unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(bucket_dir(&bucket).join(format!("f{i}.txt")))
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(i))
            .unwrap();
    }

    evict(&bucket_dir(&bucket), EvictionPolicy::MaxCount(3));

    let remaining: Vec<_> = std::fs::read_dir(bucket_dir(&bucket))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(remaining.len(), 3);
    assert!(!remaining.iter().any(|n| n == "f0.txt"));
    assert!(!remaining.iter().any(|n| n == "f1.txt"));
    assert!(remaining.iter().any(|n| n == "f4.txt"));

    let _ = std::fs::remove_dir_all(bucket_dir(&bucket));
}

#[test]
fn test_max_bytes_eviction() {
    let bucket = unique_bucket();
    for i in 0..3 {
        save(
            &bucket,
            &format!("f{}.bin", i),
            &vec![0u8; 1000],
            EvictionPolicy::MaxBytes(100_000),
        )
        .unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(bucket_dir(&bucket).join(format!("f{i}.bin")))
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(i))
            .unwrap();
    }

    evict(&bucket_dir(&bucket), EvictionPolicy::MaxBytes(2500));

    let remaining: Vec<_> = std::fs::read_dir(bucket_dir(&bucket))
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(remaining.len(), 2);

    let _ = std::fs::remove_dir_all(bucket_dir(&bucket));
}

#[test]
fn test_evict_empty_dir_no_panic() {
    let bucket = unique_bucket();
    let dir = bucket_dir(&bucket);
    std::fs::create_dir_all(&dir).unwrap();
    evict(&dir, EvictionPolicy::MaxCount(10));
    evict(&dir, EvictionPolicy::MaxBytes(1000));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_evict_nonexistent_dir_no_panic() {
    let dir = std::env::temp_dir().join(format!("nonexistent-{}", uuid::Uuid::new_v4()));
    evict(&dir, EvictionPolicy::MaxCount(10));
}

#[test]
fn test_materialize_dropped_temp_screenshot_to_durable_path() {
    let temp_root = std::env::temp_dir().join(format!("codeterm-source-{}", uuid::Uuid::new_v4()));
    let temp_dir = temp_root
        .join("TemporaryItems")
        .join("NSIRD_screencaptureui_abc123");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let source = temp_dir.join("Screen Recording 2026-10-22.mov");
    std::fs::write(&source, b"screenshot").unwrap();
    let materialized = materialize_dropped_path(&source).unwrap();

    assert!(materialized.exists());
    assert_ne!(materialized, source);
    assert_eq!(std::fs::read(&materialized).unwrap(), b"screenshot");
    assert!(materialized.starts_with(bucket_dir(CHAT_BUCKET)));

    let _ = std::fs::remove_file(&materialized);
    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn test_non_temp_safe_ascii_path_is_copied() {
    let source = std::env::temp_dir().join(format!("normal-drop-{}.txt", uuid::Uuid::new_v4()));
    std::fs::write(&source, b"normal").unwrap();
    let referenced = materialize_dropped_path(&source).unwrap();
    assert_ne!(referenced, source);
    std::fs::remove_file(&source).unwrap();
    assert_eq!(std::fs::read(&referenced).unwrap(), b"normal");
    std::fs::remove_file(referenced).unwrap();
    let _ = std::fs::remove_file(source);
}

#[test]
fn test_unsafe_unicode_path_maps_to_safe_chat_path() {
    let dir = std::env::temp_dir().join(format!("codeterm-src-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("Снимок экрана — 2026-06-23 в 14.07.27.png");
    std::fs::write(&source, b"img").unwrap();

    let safe = materialize_dropped_path(&source).unwrap();
    let name = safe.file_name().unwrap().to_string_lossy().to_string();

    assert!(safe.starts_with(bucket_dir(CHAT_BUCKET)));
    assert!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-')),
        "basename not safe ASCII: {name}"
    );
    assert!(name.ends_with(".png"), "ext not preserved: {name}");
    assert!(std::fs::metadata(&safe).is_ok(), "safe path unreadable");

    let _ = std::fs::remove_file(&safe);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_extension_lowercased_and_preserved() {
    let dir = std::env::temp_dir().join(format!("codeterm-src-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("файл копия.JPG");
    std::fs::write(&source, b"x").unwrap();

    let safe = materialize_dropped_path(&source).unwrap();
    let name = safe.file_name().unwrap().to_string_lossy().to_string();
    assert!(name.ends_with(".jpg"), "ext should be lowercased: {name}");

    let _ = std::fs::remove_file(&safe);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_chat_bucket_sources_are_copied_again() {
    let dir = std::env::temp_dir().join(format!("codeterm-src-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("отчёт.pdf");
    std::fs::write(&source, b"pdf").unwrap();

    let safe = materialize_dropped_path(&source).unwrap();
    let again = materialize_dropped_path(&safe).unwrap();
    assert_ne!(safe, again);
    assert_eq!(std::fs::read(&again).unwrap(), b"pdf");
    std::fs::remove_file(again).unwrap();

    let _ = std::fs::remove_file(&safe);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_large_stable_file_is_copied() {
    let dir = std::env::temp_dir().join(format!("codeterm-src-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("большой файл.bin");
    std::fs::write(&source, vec![7u8; 4 * 1024 * 1024 + 1024]).unwrap();

    let safe = materialize_dropped_path(&source).unwrap();
    assert!(safe.starts_with(bucket_dir(CHAT_BUCKET)));
    let size = source_len(&source);
    std::fs::write(&source, b"changed").unwrap();
    assert_eq!(std::fs::read(&safe).unwrap(), vec![7u8; size]);
    assert!(std::fs::symlink_metadata(&safe)
        .unwrap()
        .file_type()
        .is_file());

    let _ = std::fs::remove_file(&safe);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_dropped_folder_keeps_its_path_and_kind() {
    let dir = std::env::temp_dir().join(format!("codeterm-папка-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("notes.txt");
    std::fs::write(&file, b"x").unwrap();

    assert_eq!(dropped_path_kind(&dir), "folder");
    assert_eq!(dropped_path_kind(&file), "file");
    assert_eq!(dropped_path_kind(&dir.join("absent")), "missing");
    assert_eq!(materialize_dropped_path(&dir).unwrap(), dir);

    let _ = std::fs::remove_dir_all(dir);
}

fn source_len(p: &Path) -> usize {
    std::fs::metadata(p).unwrap().len() as usize
}

#[test]
fn empty_byte_upload_is_refused_before_storage() {
    let bucket = unique_bucket();
    assert_eq!(
        save(&bucket, "empty", &[], EvictionPolicy::MaxCount(10)),
        Err("Empty file".into())
    );
    assert!(!bucket_dir(&bucket).exists());
}

#[test]
fn publishing_an_attachment_never_evicts_that_same_file() {
    let bucket = unique_bucket();
    let path = save(
        &bucket,
        "oversized.bin",
        b"complete",
        EvictionPolicy::MaxBytes(1),
    )
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"complete");
    std::fs::remove_dir_all(bucket_dir(&bucket)).unwrap();
}
