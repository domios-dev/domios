use crate::schema::{validate_identity, APPLICATION_ID, SCHEMA_VERSION};
use crate::StorageError;
use rusqlite::Connection;

#[test]
fn startup_accepts_only_the_1_17_schema_without_upgrading() {
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    for version in -1..=SCHEMA_VERSION + 1 {
        conn.pragma_update(None, "user_version", version).unwrap();
        let actual: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        if actual == SCHEMA_VERSION {
            assert_eq!(validate_identity(&conn).unwrap(), SCHEMA_VERSION);
        } else {
            assert!(matches!(
                validate_identity(&conn),
                Err(StorageError::Incompatible { .. })
            ));
        }
        let after: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(after, actual);
    }
}
