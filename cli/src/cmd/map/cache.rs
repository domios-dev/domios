use crate::cmd::map::store::Store;
use std::path::Path;
use std::time::SystemTime;

pub fn fs_mtime_ns(path: &Path) -> std::io::Result<i64> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta.modified().or_else(|_| meta.created())?;
    let dur = mtime.duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(dur.as_nanos() as i64)
}

pub fn fs_size(path: &Path) -> std::io::Result<i64> {
    Ok(std::fs::metadata(path)?.len() as i64)
}

pub struct CacheCheck {
    pub fresh: bool,
}

pub fn check_file(store: &Store, rel_path: &str, fs_mtime: i64) -> rusqlite::Result<CacheCheck> {
    let row = store
        .conn
        .query_row(
            "SELECT mtime_ns, indexed FROM file WHERE path = ?1",
            rusqlite::params![rel_path],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )
        .ok();
    Ok(CacheCheck {
        fresh: matches!(row, Some((stored, indexed)) if indexed == 1 && stored == fs_mtime),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_after_upsert_with_same_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path()).unwrap();
        let _id = store.upsert_file("foo.rs", Some("rust"), 1234, 99).unwrap();
        let check = check_file(&store, "foo.rs", 1234).unwrap();
        assert!(check.fresh);
        let stale = check_file(&store, "foo.rs", 9999).unwrap();
        assert!(!stale.fresh);
        let missing = check_file(&store, "absent.rs", 1).unwrap();
        assert!(!missing.fresh);
    }
}
