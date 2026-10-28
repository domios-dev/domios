use super::*;
use codeterm_mem::backend::{contract, MemStore, SqliteMemStore, StoreTable, WriteContext};
use codeterm_mem::store::MemAuthor;

mod engines;
mod maintenance;
mod transfer;

struct Fixture {
    url: String,
    options: PostgresOptions,
    store: Option<PostgresMemStore>,
}

impl Fixture {
    fn new() -> Self {
        let url = std::env::var("DOMIOS_TEST_POSTGRES_URL").expect(
            "Postgres fixture requires DOMIOS_TEST_POSTGRES_URL; run through nextest mem-postgres",
        );
        let options = PostgresOptions {
            schema: format!("mem_test_{}", uuid::Uuid::new_v4().simple()),
            ..Default::default()
        };
        let store = PostgresMemStore::connect(&url, options.clone())
            .unwrap_or_else(|_| panic!("cannot open disposable Postgres fixture"));
        Self {
            url,
            options,
            store: Some(store),
        }
    }

    fn store(&self) -> &PostgresMemStore {
        self.store
            .as_ref()
            .expect("fixture store exists until teardown")
    }

    fn client(&self) -> postgres::Client {
        postgres::Client::connect(&self.url, NoTls)
            .unwrap_or_else(|_| panic!("cannot connect to disposable Postgres fixture"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.store.take();
        let result = self
            .client()
            .batch_execute(&format!("DROP SCHEMA {} CASCADE", self.options.schema));
        if !std::thread::panicking() {
            assert!(result.is_ok(), "disposable Postgres schema cleanup failed");
        }
    }
}

macro_rules! contract_case {
    ($name:ident, $contract:ident) => {
        #[test]
        #[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL; nextest mem-postgres names and runs this case"]
        fn $name() {
            let fixture = Fixture::new();
            contract::$contract(fixture.store());
        }
    };
}

contract_case!(postgres_save_replace_history, save_replace_history);
contract_case!(postgres_fts_hit, fts_hit);
contract_case!(postgres_vector_stamp_refusal, vector_stamp_refusal);
contract_case!(postgres_graph_relate_unrelate, graph_relate_unrelate);
contract_case!(postgres_area_merge_attribution, area_merge_attribution);
contract_case!(postgres_archive, archive);
contract_case!(
    postgres_guarded_replace_is_atomic,
    guarded_replace_is_atomic
);

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn postgres_export_import_round_trip() {
    let source = Fixture::new();
    let target = Fixture::new();
    contract::export_import_round_trip(source.store(), target.store());
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn sqlite_postgres_sqlite_round_trip() {
    let source_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let source = SqliteMemStore::open_in_memory(source_dir.path()).unwrap();
    let target = Fixture::new();
    contract::export_import_round_trip(&source, target.store());
    let restored = SqliteMemStore::open_in_memory(target_dir.path()).unwrap();
    let pages = export_all(target.store());
    for page in &pages {
        MemStore::import_page(&restored, &WriteContext::system("round-trip"), page).unwrap();
    }
    assert_eq!(
        restored.table_counts().unwrap(),
        source.table_counts().unwrap()
    );
    assert_eq!(export_all(&restored), export_all(&source));
}

fn export_all(store: &dyn MemStore) -> Vec<codeterm_mem::backend::ExportPage> {
    let mut cursor = None;
    let mut pages = Vec::new();
    loop {
        let page = store.export_page(cursor.as_ref(), 1).unwrap();
        cursor = page.next.clone();
        pages.push(page);
        if cursor.is_none() {
            return pages;
        }
    }
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn postgres_read_only_open_and_version_refusal() {
    let fixture = Fixture::new();
    let ctx = WriteContext::system("read-only-contract");
    let id = fixture
        .store()
        .save(&ctx, &contract::item("read only source"), false)
        .unwrap()
        .id;
    let before = export_all(fixture.store());
    let readonly = open_for_test(&fixture.url, fixture.options.clone().read_only()).unwrap();
    assert!(readonly.get(&id).unwrap().is_some());
    assert_eq!(export_all(readonly.as_ref()), before);
    assert!(readonly
        .save(&ctx, &contract::item("write refused"), false)
        .unwrap_err()
        .to_string()
        .contains("postgres_memory_read_only (25006)"));
    assert_eq!(export_all(fixture.store()), before);
    let mut client = fixture.client();
    client
        .batch_execute(&format!(
            "UPDATE {}.mem_schema_version SET version=0",
            fixture.options.schema
        ))
        .unwrap();
    let result = open_for_test(&fixture.url, fixture.options.clone().read_only());
    assert!(matches!(
        result,
        Err(PostgresOpenError::SchemaVersionMismatch {
            found: Some(0),
            expected: migrations::VERSION
        })
    ));
    let version: i64 = client
        .query_one(
            &format!(
                "SELECT version FROM {}.mem_schema_version",
                fixture.options.schema
            ),
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(version, 0);
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn postgres_migrations_second_open_is_noop() {
    let fixture = Fixture::new();
    fixture
        .store()
        .save(
            &WriteContext::new(MemAuthor::owner(None), "migration"),
            &contract::item("survives reopen"),
            false,
        )
        .unwrap();
    let before = export_all(fixture.store());
    let mut client = fixture.client();
    let before_txid: String = client
        .query_one(
            &format!(
                "SELECT xmin::text FROM {}.mem_schema_version",
                fixture.options.schema
            ),
            &[],
        )
        .unwrap()
        .get(0);
    let reopened = PostgresMemStore::connect(&fixture.url, fixture.options.clone()).unwrap();
    assert_eq!(export_all(&reopened), before);
    let after_txid: String = client
        .query_one(
            &format!(
                "SELECT xmin::text FROM {}.mem_schema_version",
                fixture.options.schema
            ),
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(before_txid, after_txid);
    assert_eq!(
        reopened.table_counts().unwrap().len(),
        StoreTable::ALL.len()
    );
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn postgres_wake_up_excludes_neighbor_and_preserves_priority() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let neighbor = WriteContext::new(
        MemAuthor::neighbor("friend".into(), "friendship".into()),
        "neighbor",
    );
    let mut item = contract::item("neighbor-high-importance");
    item.importance = 1000.0;
    let id = store.save(&neighbor, &item, false).unwrap().id;
    item.title = "neighbor-pinned-refused".into();
    item.pinned = true;
    assert!(store.save(&neighbor, &item, false).is_err());
    let owner = WriteContext::new(MemAuthor::owner(None), "owner");
    assert!(store
        .update(
            &owner,
            &id,
            &codeterm_mem::store::MemUpdate {
                pinned: Some(true),
                ..Default::default()
            }
        )
        .is_err());
    let mut client = fixture.client();
    client.batch_execute(&format!("ALTER TABLE {0}.mem_items DROP CONSTRAINT neighbor_instructions; ALTER TABLE {0}.mem_items DISABLE TRIGGER mem_neighbor_update_guard",fixture.options.schema)).unwrap();
    client
        .execute(
            &format!(
                "UPDATE {}.mem_items SET is_pinned_wakeup=1 WHERE id=$1",
                fixture.options.schema
            ),
            &[&id],
        )
        .unwrap();
    let mut pin = contract::item("owner-pin");
    pin.pinned = true;
    store.save(&owner, &pin, false).unwrap();
    let ordinary = contract::item("owner-ranked");
    store.save(&owner, &ordinary, false).unwrap();
    let mut rule = contract::item("owner-rule");
    rule.kind = codeterm_mem::MemKind::Rule;
    store.save(&owner, &rule, false).unwrap();
    let text = store
        .wake_up(&codeterm_mem::WakeUpOptions {
            l1_budget_tokens: 2000,
            ..Default::default()
        })
        .unwrap();
    assert!(!text.contains("neighbor-high-importance"));
    assert!(!text.contains("neighbor-pinned-refused"));
    assert!(text.find("owner-rule").unwrap() < text.find("owner-pin").unwrap());
    assert!(text.find("owner-pin").unwrap() < text.find("owner-ranked").unwrap());
}

#[test]
fn production_open_refuses_incomplete_backend() {
    assert!(matches!(
        open("never-connect", PostgresOptions::default()),
        Err(PostgresOpenError::Incomplete)
    ));
}

#[test]
fn options_reject_schema_injection_and_errors_redact_url() {
    assert!(!valid_schema("public"));
    assert!(!valid_schema("memory; DROP SCHEMA public"));
    assert!(!valid_schema("pg_catalog"));
    assert!(valid_schema("memory_123"));
    let secret = "postgres://username:do-not-print@invalid.invalid/db";
    let result = PostgresMemStore::connect(
        secret,
        PostgresOptions {
            schema: "invalid;schema".into(),
            ..Default::default()
        },
    );
    assert!(matches!(result, Err(PostgresOpenError::InvalidOptions)));
    let error = PostgresOpenError::InvalidUrl.to_string();
    assert!(!error.contains(secret));
    assert!(!error.contains("do-not-print"));
}
