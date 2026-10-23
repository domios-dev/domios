use crate::assets::{self, Layout};
use crate::schema::{blueprint_calls, open_existing, MIGRATIONS};
use crate::{Storage, StorageConfig};
use rusqlite::{params, Connection, OpenFlags};

fn version_before_run_identity() -> i32 {
    MIGRATIONS
        .iter()
        .position(|migration| *migration == blueprint_calls::RUN_IDENTITY_SCHEMA)
        .unwrap() as i32
}
fn raw(root: &std::path::Path) -> Connection {
    let layout = Layout::of_root(&assets::canonical_dir(root).unwrap());
    open_existing(&layout.database, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap()
}
#[test]
fn blueprint_v2_surface_upgrade_removes_only_old_graphs_and_history_and_reopens_v2() {
    let root = std::env::temp_dir().join(format!(
        "blueprint-v2-storage-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    Storage::create_at_schema_version(&root, version_before_run_identity()).unwrap();
    let conn = raw(&root);
    conn.execute("INSERT INTO workspaces(id,ordinal,name,protected,task_key_next,revision) VALUES ('unrelated',0,'Keep workspace',0,0,1)",[]).unwrap();
    let document = serde_json::json!({"schema_version":2,"kind":"watch","name":"retained-watch","steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"echo ready"}},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"ready?"}},"buckets":["ready"],"matches":{"ready":{"lit":"ready"}}}]});
    for (id, ordinal, doc) in [
        (
            "old",
            0,
            Some(serde_json::json!({"schema_version":1,"watch":{}}).to_string()),
        ),
        ("v2", 1, Some(document.to_string())),
    ] {
        conn.execute("INSERT INTO blueprints(id,ordinal,creator_tab_id,workspace_id,cwd,agent_authored,revision) VALUES (?1,?2,'author','workspace','.',0,1)",params![id,ordinal]).unwrap();
        conn.execute("INSERT INTO blueprint_definitions(blueprint_id,version,provenance,name,source_kind,probe,question,interval_ms,max_transitions,max_actions,created_revision,document_json) VALUES (?1,1,'authored','retained-watch','probe','echo ready','ready?',20000,8,8,1,?2)",params![id,doc]).unwrap();
        conn.execute("INSERT INTO blueprint_heads VALUES (?1,1)", [id])
            .unwrap();
        conn.execute("INSERT INTO blueprint_buckets(blueprint_id,version,ordinal,bucket) VALUES (?1,1,0,'ready')",[id]).unwrap();
        conn.execute("INSERT INTO blueprint_step_journal(blueprint_id,run_id,step_index,step_name,state,started_at_ms,ended_at_ms) VALUES (?1,'run',0,'source','done',1,2)",[id]).unwrap();
    }
    conn.execute("INSERT INTO blueprint_definitions(blueprint_id,version,provenance,name,source_kind,probe,question,interval_ms,max_transitions,max_actions,created_revision) VALUES ('old',2,'authored','old history','probe','echo old','old?',20000,8,8,1)",[]).unwrap();
    conn.execute("INSERT INTO blueprint_definitions(blueprint_id,version,provenance,name,source_kind,probe,question,interval_ms,max_transitions,max_actions,created_revision) VALUES ('v2',2,'authored','obsolete history','probe','echo old','old?',20000,8,8,1)",[]).unwrap();
    conn.execute("INSERT INTO blueprint_transitions(blueprint_id,ordinal,transition_id,bucket,accepted_at,terminal,definition_version) VALUES ('v2',0,'obsolete-transition','ready',1,0,2)",[]).unwrap();
    drop(conn);
    let storage = Storage::open(&root, StorageConfig::default()).unwrap();
    storage
        .shutdown()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();
    for _ in 0..2 {
        let storage = Storage::open(&root, StorageConfig::default()).unwrap();
        let conn = raw(&root);
        assert_eq!(
            conn.query_row("SELECT count(*) FROM blueprints", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT document_json FROM blueprint_definitions WHERE blueprint_id='v2'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            document.to_string()
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM blueprint_definitions WHERE blueprint_id='v2'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM blueprint_transitions WHERE blueprint_id='v2'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT version FROM blueprint_heads WHERE blueprint_id='v2'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        for table in [
            "blueprint_definitions",
            "blueprint_buckets",
            "blueprint_heads",
            "blueprint_step_journal",
        ] {
            assert_eq!(
                conn.query_row(
                    &format!("SELECT count(*) FROM {table} WHERE blueprint_id='old'"),
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
        }
        assert_eq!(
            conn.query_row(
                "SELECT name FROM workspaces WHERE id='unrelated'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "Keep workspace"
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            0
        );
        drop(conn);
        storage
            .shutdown()
            .wait(std::time::Duration::from_secs(30))
            .unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn blueprint_v2_check_upgrade_keeps_obsolete_definition_receipt() {
    let root = std::env::temp_dir().join(format!(
        "blueprint-v2-receipt-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    Storage::create_at_schema_version(&root, version_before_run_identity()).unwrap();
    let conn = raw(&root);
    conn.execute("INSERT INTO blueprints(id,ordinal,creator_tab_id,workspace_id,cwd,agent_authored,revision) VALUES ('old',0,'author','workspace','.',0,1)", []).unwrap();
    conn.execute("INSERT INTO blueprint_definitions(blueprint_id,version,provenance,name,source_kind,probe,question,interval_ms,max_transitions,max_actions,created_revision) VALUES ('old',1,'authored','obsolete','probe','opaque-old-command','old?',20000,8,8,1)", []).unwrap();
    drop(conn);
    let storage = Storage::open(&root, StorageConfig::default()).unwrap();
    storage
        .shutdown()
        .wait(std::time::Duration::from_secs(30))
        .unwrap();
    let conn = raw(&root);
    let receipt: (String, String) = conn
        .query_row(
            "SELECT blueprint_id,probe FROM blueprint_v1_removal_receipts",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(receipt, ("old".into(), "opaque-old-command".into()));
    assert_eq!(
        conn.query_row("SELECT count(*) FROM blueprints", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(conn);
    std::fs::remove_dir_all(root).unwrap();
}
