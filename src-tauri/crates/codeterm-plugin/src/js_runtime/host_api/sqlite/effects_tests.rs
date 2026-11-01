use super::*;
use crate::js_runtime::{
    host_api::PluginSandbox,
    vm::{call_json_interruptible, PluginVm},
};
use codeterm_decision::physical::RequestScope;
use serde_json::{json, Value};
use std::{sync::Arc, time::Instant};

const FIXTURE: &str = r#"module.exports={
    sync(db,sql,params){var rows=host.sqliteQueryAllJson(db,sql,JSON.stringify(params));return rows==null?{refused:true}:{rows:JSON.parse(rows),refused:false};},
    async(db,sql,params){var ticket=JSON.parse(host.sqliteQueryStart(db,sql,JSON.stringify(params)));if(!ticket.jobId)return {refused:true};return host.awaitJob(ticket.jobId,function(result){return {refused:!Array.isArray(result.rows),rows:result.rows};});}
};"#;

#[test]
fn durable_media_observation_sqlite_sync_and_async_cannot_create_or_attach_files_or_mutate_policy()
{
    let source = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let database = source.path().join("source.sqlite");
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute_batch("CREATE TABLE source(value TEXT); INSERT INTO source VALUES('retained');")
        .unwrap();
    let foreign = outside.path().join("ungranted.sqlite");
    rusqlite::Connection::open(&foreign)
        .unwrap()
        .execute_batch("CREATE TABLE secret(value TEXT); INSERT INTO secret VALUES('foreign');")
        .unwrap();
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![source.path().to_path_buf()], vec![])
            .with_identity("lookup-sql-policy"),
    );
    let vm = PluginVm::from_source_sandboxed("lookup-sql-policy", FIXTURE.into(), None, sandbox)
        .unwrap();
    let _lease = lease_plugin_sqlite("lookup-sql-policy");
    for method in ["sync", "async"] {
        let output = outside.path().join(format!("vacuum-{method}.sqlite"));
        let attached = outside.path().join(format!("attached-{method}.sqlite"));
        let deadline = Instant::now() + Duration::from_secs(5);
        let _deny = RequestScope::observation_only(deadline);
        let invoke = |sql: &str, params: Value| -> Value {
            call_json_interruptible(
                &vm,
                method,
                &[json!(database), json!(sql), params],
                Default::default(),
                deadline,
            )
            .unwrap()
        };
        for (sql, params) in [
            ("VACUUM INTO ?1", json!([output])),
            ("ATTACH DATABASE ?1 AS escaped", json!([foreign])),
            ("ATTACH DATABASE ?1 AS escaped", json!([attached])),
            ("DETACH DATABASE main", json!([])),
            ("PRAGMA query_only=OFF", json!([])),
            ("PRAGMA writable_schema=ON", json!([])),
            ("PRAGMA user_version=7", json!([])),
            ("INSERT INTO source VALUES('changed')", json!([])),
        ] {
            assert_eq!(invoke(sql, params)["refused"], true, "{method}: {sql}");
        }
        assert!(
            !output.exists(),
            "{method}: VACUUM output must not be created"
        );
        assert!(
            !attached.exists(),
            "{method}: ATTACH output must not be created"
        );
        assert_eq!(
            invoke(
                "WITH selected AS (SELECT value FROM source) SELECT value FROM selected",
                json!([])
            )["rows"],
            json!([{"value":"retained"}])
        );
        assert_eq!(
            invoke("SELECT value FROM json_each('[1,2]')", json!([]))["rows"],
            json!([{"value":1},{"value":2}])
        );
        assert_eq!(
            invoke("PRAGMA query_only", json!([]))["rows"],
            json!([{"query_only":1}])
        );
        assert_eq!(
            invoke("PRAGMA user_version", json!([]))["rows"],
            json!([{"user_version":0}])
        );
        assert!(invoke("PRAGMA database_list", json!([]))["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["name"] != "escaped"));
    }
    let connection = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM source", [], |row| row
                .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        0
    );
}
