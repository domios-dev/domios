use super::*;

fn entry(id: usize) -> storage::Entry {
    storage::Entry {
        file_id: format!("{id:032x}"),
        message_id: "b".repeat(32),
        name: format!("report-{id}.txt"),
        mime: "text/plain".into(),
        size_bytes: 6,
        direction: "in".into(),
        state: "saved".into(),
    }
}

#[tokio::test]
async fn many_file_listing_resolves_the_single_loaded_index_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = Metadata::new(temp.path().to_owned(), "recipient");
    let friendship = "a".repeat(64);
    let folder = metadata.friend_folder(&friendship).unwrap();
    std::fs::create_dir_all(&folder).unwrap();
    let entries: Vec<_> = (0..64).map(entry).collect();
    for entry in &entries {
        std::fs::write(folder.join(entry.filename()), b"opaque").unwrap();
    }
    let index = format!("files-{friendship}.json");
    metadata.write(index.clone(), entries).await.unwrap();
    let loaded = metadata.file_entries(&friendship).await.unwrap();
    let account_folder = folder.parent().unwrap().parent().unwrap();
    std::fs::remove_file(account_folder.join(index)).unwrap();

    let view = concierge_file_views(&metadata, &friendship, &loaded).await;
    let files = view["files"].as_array().unwrap();
    assert_eq!(files.len(), 64);
    for (id, file) in files.iter().enumerate() {
        assert_eq!(file["file_id"], format!("{id:032x}"));
        assert_eq!(std::fs::read(file["path"].as_str().unwrap()).unwrap(), b"opaque");
    }
    let path = metadata.saved_file(&friendship, &loaded[7]).await.unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"opaque");
}

#[tokio::test]
async fn entry_path_resolution_refuses_out_of_folder_ids_and_unsaved_files() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = Metadata::new(temp.path().to_owned(), "recipient");
    let friendship = "a".repeat(64);
    let mut outside = entry(1);
    outside.file_id = "../../outside".into();
    let mut unsaved = entry(2);
    unsaved.state = "downloading".into();
    let mut outgoing = entry(3);
    outgoing.direction = "out".into();
    for entry in [outside, unsaved, outgoing] {
        assert!(matches!(metadata.saved_file(&friendship, &entry).await, Err(Error::InvalidRequest)));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn listing_and_entry_resolution_refuse_symlinks_inside_and_outside_the_folder() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = Metadata::new(temp.path().to_owned(), "recipient");
    let friendship = "a".repeat(64);
    let folder = metadata.friend_folder(&friendship).unwrap();
    std::fs::create_dir_all(&folder).unwrap();
    let inside = folder.join("inside.txt");
    let outside = temp.path().join("outside.txt");
    std::fs::write(&inside, b"inside").unwrap();
    std::fs::write(&outside, b"outside").unwrap();
    let entries = vec![entry(1), entry(2)];
    for (entry, target) in entries.iter().zip([inside, outside]) {
        std::os::unix::fs::symlink(target, folder.join(entry.filename())).unwrap();
        assert!(matches!(metadata.saved_file(&friendship, entry).await, Err(Error::InvalidRequest)));
    }
    let view = concierge_file_views(&metadata, &friendship, &entries).await;
    assert_eq!(view["files"], json!([]));
}
