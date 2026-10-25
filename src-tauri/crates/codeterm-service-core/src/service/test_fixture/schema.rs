use std::path::Path;
use std::sync::OnceLock;

pub(super) fn seed(root: &Path) {
    static DATABASES: OnceLock<[Vec<u8>; 2]> = OnceLock::new();
    let schemas = DATABASES.get_or_init(|| {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        drop(crate::service::store::ServicesStore::open(&root.join("services.db")).unwrap());
        drop(crate::usage::UsageStore::open(root).unwrap());
        [
            std::fs::read(root.join("services.db")).unwrap(),
            std::fs::read(root.join("usage/usage.db")).unwrap(),
        ]
    });
    for (path, bytes) in ["services.db", "usage/usage.db"].into_iter().zip(schemas) {
        let path = root.join(path);
        if path.exists() {
            continue;
        }
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        std::io::Write::write_all(&mut file, bytes).unwrap();
    }
}

#[cfg(test)]
mod tests;
