use crate::assets::Layout;
use crate::{Storage, StorageConfig};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

pub fn seed_empty_root(root: &Path) {
    static SCHEMA: OnceLock<Vec<u8>> = OnceLock::new();
    let schema = SCHEMA.get_or_init(|| {
        let template = tempfile::tempdir().expect("storage schema template directory");
        let storage = Storage::open(template.path(), StorageConfig::default())
            .expect("migrate storage schema template");
        storage
            .shutdown()
            .wait(Duration::from_secs(30))
            .expect("close storage schema template");
        std::fs::read(Layout::of_root(template.path()).database).expect("read closed storage schema")
    });
    let directory = root.join("storage");
    std::fs::create_dir_all(root).expect("storage fixture root");
    std::fs::create_dir(&directory).expect("storage fixture must be fresh");
    std::fs::write(Layout::of_root(root).database, schema).expect("copy storage schema template");
}
