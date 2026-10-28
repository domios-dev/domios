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

async fn open_fixture() -> (
    crate::service::test_fixture::ServiceFixture,
    TabCaller,
    String,
    String,
    std::path::PathBuf,
) {
    let (mut mux, general) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let concierge = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&concierge).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("opaque-session".into());
    tab.session_binding_nonce = Some("opaque-nonce".into());
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let friend = "a".repeat(64);
    let file_id = "1".repeat(32);
    fixture
        .seed_neighbor_concierge(
            &concierge,
            &friend,
            Permission {
                allowed: true,
                scope: "opaque-scope".into(),
                ..Default::default()
            },
        )
        .await;
    let path = fixture
        .seed_neighbor_saved_file(&friend, &file_id, b"opaque")
        .await
        .canonicalize()
        .unwrap();
    let caller = TabCaller {
        tab_id: concierge,
        binding_nonce: "opaque-nonce".into(),
    };
    (fixture, caller, friend, file_id, path)
}

#[tokio::test]
async fn owner_open_can_resolve_without_launching_and_keeps_default_launch_behavior() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (fixture, _, friend, file_id, path) = open_fixture().await;
    assert!(path.is_absolute());
    for launch in [Some(false), None, Some(true)] {
        let mut request = json!({"operation":"open","friendship_id":friend,"file_id":file_id});
        if let Some(launch) = launch {
            request["launch"] = json!(launch);
        }
        let command: Command = serde_json::from_value(request).unwrap();
        let launches = AtomicUsize::new(0);
        let result = fixture
            .service
            .neighbor_file_local_with(&command, None, |opened| {
                assert_eq!(opened, path);
                launches.fetch_add(1, Ordering::Relaxed);
                std::future::ready(Ok(()))
            })
            .await
            .unwrap();
        if launch == Some(false) {
            assert_eq!(
                result,
                json!({"opened":false,"path":path.to_string_lossy()})
            );
            assert_eq!(launches.load(Ordering::Relaxed), 0);
        } else {
            assert_eq!(result, json!({"opened":true}));
            assert_eq!(launches.load(Ordering::Relaxed), 1);
        }
    }
}

#[tokio::test]
async fn owner_path_only_open_refuses_unknown_and_foreign_file_handles() {
    let (fixture, _, friend, file_id, _) = open_fixture().await;
    let foreign = "b".repeat(64);
    let foreign_file = "2".repeat(32);
    fixture
        .seed_neighbor_saved_file(&foreign, &foreign_file, b"foreign")
        .await;
    for (friendship, file_id) in [
        (&friend, "3".repeat(32)),
        (&friend, foreign_file),
        (&foreign, file_id),
    ] {
        let command: Command = serde_json::from_value(json!({
            "operation":"open","friendship_id":friendship,"file_id":file_id,"launch":false
        }))
        .unwrap();
        let error = fixture
            .service
            .neighbor_file_local_with(&command, None, |_| std::future::ready(Err(Error::Gateway)))
            .await
            .unwrap_err();
        assert_eq!(error, Error::InvalidRequest.to_string());
    }
}

#[tokio::test]
async fn concierge_open_keeps_saved_metadata_and_never_launches_for_any_launch_flag() {
    let (fixture, caller, friend, file_id, path) = open_fixture().await;
    for launch in [Some(false), None, Some(true)] {
        let command = Command::Open {
            friendship_id: friend.clone(),
            file_id: file_id.clone(),
            launch,
        };
        let result = fixture
            .service
            .neighbor_file_local_with(&command, Some(&caller), |_| {
                std::future::ready(Err(Error::Gateway))
            })
            .await
            .unwrap();
        assert_eq!(result["file_id"], file_id);
        assert_eq!(result["state"], "saved");
        assert_eq!(result["path"], path.to_string_lossy().as_ref());
        assert!(result.get("opened").is_none());
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
        assert_eq!(
            std::fs::read(file["path"].as_str().unwrap()).unwrap(),
            b"opaque"
        );
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
        assert!(matches!(
            metadata.saved_file(&friendship, &entry).await,
            Err(Error::InvalidRequest)
        ));
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
        assert!(matches!(
            metadata.saved_file(&friendship, entry).await,
            Err(Error::InvalidRequest)
        ));
    }
    let view = concierge_file_views(&metadata, &friendship, &entries).await;
    assert_eq!(view["files"], json!([]));
}
