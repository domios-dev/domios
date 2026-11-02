use std::path::Path;
use std::sync::OnceLock;

pub(super) fn seed(root: &Path) {
    static USAGE_DATABASE: OnceLock<Vec<u8>> = OnceLock::new();
    let bytes = USAGE_DATABASE.get_or_init(|| {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        drop(crate::usage::UsageStore::open(root).unwrap());
        std::fs::read(root.join("usage/usage.db")).unwrap()
    });
    let path = root.join("usage/usage.db");
    if path.exists() {
        return;
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    std::io::Write::write_all(&mut file, bytes).unwrap();
}
