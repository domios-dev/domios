use super::{check_integrity, open_existing, pragma_value, APPLICATION_ID, SCHEMA_VERSION};
use crate::assets::{self, Layout};
use crate::error::{sqlite_error, StorageError};
use rusqlite::{Connection, OpenFlags};

pub(super) fn older_owned_database(
    conn: &mut Connection,
    layout: &Layout,
) -> Result<(), StorageError> {
    let application_id = pragma_value::<i32>(conn, "application_id")?;
    let version = pragma_value::<i32>(conn, "user_version")?;
    if application_id != APPLICATION_ID || !(0..=SCHEMA_VERSION).contains(&version) {
        return Err(StorageError::Incompatible {
            application_id,
            schema_version: version,
            supported: SCHEMA_VERSION,
        });
    }
    if version < SCHEMA_VERSION {
        check_integrity(conn)?;
        backup_before_change(conn, layout, version)?;
        super::apply_schema(conn, version)?;
    }
    Ok(())
}

fn backup_before_change(
    source: &Connection,
    layout: &Layout,
    version: i32,
) -> Result<(), StorageError> {
    let name = format!("pre-upgrade-v{version}-{:032x}.db", assets::unique_token());
    let staging = layout.tmp.join(&name);
    let destination = layout.dir.join(name);
    assets::create_owner_file(&staging)?;
    let result = (|| {
        let mut target = open_existing(&staging, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        target
            .pragma_update(None, "synchronous", "FULL")
            .map_err(|error| sqlite_error("configure upgrade backup", error))?;
        crate::backup::copy_database(source, &mut target)?;
        let mode: String = target
            .pragma_update_and_check(None, "journal_mode", "DELETE", |row| row.get(0))
            .map_err(|error| sqlite_error("detach upgrade backup from wal", error))?;
        super::require("journal_mode", "delete", &mode.to_ascii_lowercase())?;
        check_integrity(&target)?;
        target
            .close()
            .map_err(|(_, error)| sqlite_error("close upgrade backup", error))?;
        assets::sync_file(&staging)?;
        assets::publish_no_replace(&staging, &destination, &layout.dir)
    })();
    if result.is_err() {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(super::suffixed(&staging, suffix));
        }
    }
    result
}

#[cfg(test)]
mod tests;
