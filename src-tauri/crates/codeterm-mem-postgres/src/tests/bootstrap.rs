use super::*;
use std::sync::Barrier;

struct FreshDatabase {
    admin: postgres::Client,
    name: String,
    url: String,
}

impl FreshDatabase {
    fn new() -> Self {
        let url = std::env::var("DOMIOS_TEST_POSTGRES_URL").expect("disposable Postgres URL");
        let mut admin = postgres::Client::connect(&url, NoTls)
            .unwrap_or_else(|_| panic!("cannot connect to disposable Postgres server"));
        let name = format!("mem_bootstrap_{}", uuid::Uuid::new_v4().simple());
        let (server, _) = url.rsplit_once('/').expect("Postgres URL database path");
        let database_url = format!("{server}/{name}");
        admin
            .batch_execute(&format!("CREATE DATABASE {name} TEMPLATE template0"))
            .expect("create fresh disposable database");
        Self {
            admin,
            name,
            url: database_url,
        }
    }
}

impl Drop for FreshDatabase {
    fn drop(&mut self) {
        let result = self
            .admin
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name));
        if !std::thread::panicking() {
            assert!(result.is_ok(), "fresh disposable database cleanup failed");
        }
    }
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn four_schema_first_opens_share_database_bootstrap_lock() {
    let database = FreshDatabase::new();
    let mut client = postgres::Client::connect(&database.url, NoTls).unwrap();
    let installed: i64 = client
        .query_one(
            "SELECT count(*) FROM pg_extension WHERE extname='vector'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(installed, 0);
    drop(client);
    let ready = Barrier::new(4);
    let results = std::thread::scope(|threads| {
        let handles: Vec<_> = (0..4)
            .map(|index| {
                let url = &database.url;
                let ready = &ready;
                threads.spawn(move || {
                    ready.wait();
                    PostgresMemStore::connect(
                        url,
                        PostgresOptions {
                            schema: format!("first_open_{index}"),
                            ..Default::default()
                        },
                    )
                    .map(|store| store.table_counts().unwrap().len())
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results, vec![Ok(StoreTable::ALL.len()); 4]);
}
