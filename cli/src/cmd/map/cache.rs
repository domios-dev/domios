use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rusqlite::{params, Connection, OptionalExtension};

use super::extract::Tag;

pub struct Cache {
    conn: Connection,
}

impl Cache {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create cache dir: {}", e))?;
        }
        let conn = Connection::open(path).map_err(|e| format!("open cache: {}", e))?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS map_tags (
                workspace_path TEXT NOT NULL,
                file_path      TEXT NOT NULL,
                mtime          INTEGER NOT NULL,
                tags           BLOB NOT NULL,
                PRIMARY KEY (workspace_path, file_path)
            )",
            [],
        )
        .map_err(|e| format!("create map_tags: {}", e))?;
        Ok(Cache { conn })
    }

    #[allow(dead_code)]
    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS map_tags (
                workspace_path TEXT NOT NULL,
                file_path      TEXT NOT NULL,
                mtime          INTEGER NOT NULL,
                tags           BLOB NOT NULL,
                PRIMARY KEY (workspace_path, file_path)
            )",
            [],
        )
        .map_err(|e| e.to_string())?;
        Ok(Cache { conn })
    }

    pub fn get(&self, workspace: &str, file: &str, mtime: i64) -> Option<Vec<Tag>> {
        let row: Option<(i64, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT mtime, tags FROM map_tags WHERE workspace_path = ?1 AND file_path = ?2",
                params![workspace, file],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .ok()
            .flatten();
        let (cached_mtime, blob) = row?;
        if cached_mtime != mtime {
            return None;
        }
        bincode::deserialize::<Vec<Tag>>(&blob).ok()
    }

    pub fn put(&self, workspace: &str, file: &str, mtime: i64, tags: &[Tag]) -> Result<(), String> {
        let blob = bincode::serialize(tags).map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "INSERT OR REPLACE INTO map_tags (workspace_path, file_path, mtime, tags)
                 VALUES (?1, ?2, ?3, ?4)",
                params![workspace, file, mtime, blob],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn forget(&self, workspace: &str, file: &str) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM map_tags WHERE workspace_path = ?1 AND file_path = ?2",
                params![workspace, file],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

pub fn default_cache_path(data_dir: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(data_dir).join("mem").join("repo_map.db")
}

pub fn mtime_epoch(path: &Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let mt = meta.modified().ok()?;
    let dur = mt.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(dur.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tags() -> Vec<Tag> {
        vec![
            Tag {
                name: "foo".into(),
                kind: "fn".into(),
                line: 1,
                signature: "fn foo()".into(),
            },
            Tag {
                name: "Bar".into(),
                kind: "struct".into(),
                line: 7,
                signature: "struct Bar".into(),
            },
        ]
    }

    #[test]
    fn put_then_get_round_trip() {
        let cache = Cache::open_in_memory().unwrap();
        cache.put("/ws", "src/a.rs", 100, &sample_tags()).unwrap();
        let got = cache.get("/ws", "src/a.rs", 100).unwrap();
        assert_eq!(got, sample_tags());
    }

    #[test]
    fn get_returns_none_when_mtime_differs() {
        let cache = Cache::open_in_memory().unwrap();
        cache.put("/ws", "src/a.rs", 100, &sample_tags()).unwrap();
        assert!(cache.get("/ws", "src/a.rs", 200).is_none());
    }

    #[test]
    fn get_returns_none_when_workspace_differs() {
        let cache = Cache::open_in_memory().unwrap();
        cache.put("/ws", "src/a.rs", 100, &sample_tags()).unwrap();
        assert!(cache.get("/other", "src/a.rs", 100).is_none());
    }

    #[test]
    fn get_returns_none_when_file_differs() {
        let cache = Cache::open_in_memory().unwrap();
        cache.put("/ws", "src/a.rs", 100, &sample_tags()).unwrap();
        assert!(cache.get("/ws", "src/b.rs", 100).is_none());
    }

    #[test]
    fn put_overwrites_on_same_key() {
        let cache = Cache::open_in_memory().unwrap();
        cache.put("/ws", "src/a.rs", 100, &sample_tags()).unwrap();
        let new_tags = vec![Tag {
            name: "renamed".into(),
            kind: "fn".into(),
            line: 1,
            signature: "fn renamed()".into(),
        }];
        cache.put("/ws", "src/a.rs", 100, &new_tags).unwrap();
        let got = cache.get("/ws", "src/a.rs", 100).unwrap();
        assert_eq!(got, new_tags);
    }

    #[test]
    fn forget_removes_entry() {
        let cache = Cache::open_in_memory().unwrap();
        cache.put("/ws", "src/a.rs", 100, &sample_tags()).unwrap();
        cache.forget("/ws", "src/a.rs").unwrap();
        assert!(cache.get("/ws", "src/a.rs", 100).is_none());
    }

    #[test]
    fn empty_tag_list_round_trips() {
        let cache = Cache::open_in_memory().unwrap();
        cache.put("/ws", "src/a.rs", 100, &[]).unwrap();
        let got = cache.get("/ws", "src/a.rs", 100).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn mtime_epoch_for_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.rs");
        std::fs::write(&p, "fn x() {}").unwrap();
        let m = mtime_epoch(&p).unwrap();
        assert!(m > 0);
    }

    #[test]
    fn mtime_epoch_for_missing_file_is_none() {
        assert!(mtime_epoch(Path::new("/definitely/not/here.rs")).is_none());
    }
}
