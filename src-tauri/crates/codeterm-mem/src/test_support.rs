use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub(crate) fn seed_schema(data_dir: &Path) {
    static SCHEMA: OnceLock<Vec<u8>> = OnceLock::new();
    let bytes = SCHEMA.get_or_init(|| {
        crate::embeddings::ensure_extension_registered();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::store::init_schema(&conn).unwrap();
        crate::solution_metadata::initialize(&conn).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("schema.db");
        conn.execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        std::fs::read(path).unwrap()
    });
    std::fs::create_dir_all(data_dir).unwrap();
    let path = data_dir.join("knowledge.db");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    std::io::Write::write_all(&mut file, bytes).unwrap();
}

pub(crate) fn offline_directory() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    seed_schema(directory.path());
    directory
}

pub(crate) fn fixture_manifest_dir() -> &'static Path {
    static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
    DIRECTORY.get_or_init(|| {
        std::env::var_os("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
    })
}
