use super::{Cache, ModelFiles};

#[test]
fn embedder_candidates_pinned_snapshot_cache_needs_no_ref_and_cannot_follow_another_revision() {
    const REVISION: &str = "614241f622f53c4eeff9890bdc4f31cfecc418b3";
    let directory = tempfile::tempdir().unwrap();
    let files = ModelFiles {
        cache: Cache::new(directory.path().to_path_buf()),
        revision: REVISION,
    };
    let repo = "intfloat/multilingual-e5-small";
    let snapshot = directory
        .path()
        .join("models--intfloat--multilingual-e5-small/snapshots")
        .join(REVISION);
    std::fs::create_dir_all(&snapshot).unwrap();
    let config = snapshot.join("config.json");
    std::fs::write(&config, r#"{"hidden_size":384}"#).unwrap();
    assert_eq!(files.cached(repo, "config.json"), Some(config));
    files
        .cache
        .repo(files.repo(repo))
        .create_ref("another-revision")
        .unwrap();
    assert_eq!(files.cached(repo, "model.safetensors"), None);
    let wrong = ModelFiles {
        cache: Cache::new(directory.path().to_path_buf()),
        revision: "0000000000000000000000000000000000000000",
    };
    wrong
        .cache
        .repo(wrong.repo(repo))
        .create_ref(REVISION)
        .unwrap();
    assert_eq!(wrong.cached(repo, "config.json"), None);
}
