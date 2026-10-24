use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn unreadable_mesh_file_is_preserved_and_cannot_be_overwritten() {
    let directory = std::env::temp_dir().join(format!(
        "codeterm-mesh-store-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("mesh.json");
    let corrupt = b"{not-json";
    fs::write(&path, corrupt).unwrap();
    let store = FileAccountMeshStore { path: path.clone() };

    assert!(store.load().await.is_err());
    assert!(store.save(&AccountMeshRecord::default()).await.is_err());
    assert_eq!(fs::read(&path).unwrap(), corrupt);

    fs::remove_dir_all(directory).unwrap();
}
