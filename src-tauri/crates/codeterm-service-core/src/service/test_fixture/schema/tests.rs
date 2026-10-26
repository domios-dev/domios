use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn copied_fixture_databases_are_isolated_and_reopen_without_replacing_committed_rows() {
    let root = tempfile::tempdir().unwrap();
    let first = ServiceFixture::new_at(MultiplexerState::new().0, root.path());
    first.service.services_store.conn().lock().execute_batch(
        "CREATE TABLE fixture_probe (value TEXT NOT NULL); INSERT INTO fixture_probe VALUES ('committed');",
    ).unwrap();
    first.service.usage_store.with_conn(|conn| {
        conn.execute_batch("CREATE TABLE fixture_probe (value TEXT NOT NULL); INSERT INTO fixture_probe VALUES ('committed');").unwrap();
    });
    drop(first);
    let reopened = ServiceFixture::new_at(MultiplexerState::new().0, root.path());
    let other = ServiceFixture::new(MultiplexerState::new().0);
    for (fixture, expected) in [(&reopened, 1), (&other, 0)] {
        assert_eq!(
            fixture
                .service
                .services_store
                .conn()
                .lock()
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name = 'fixture_probe'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            expected
        );
        fixture.service.usage_store.with_conn(|conn| {
            assert_eq!(
                conn.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name = 'fixture_probe'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                expected
            );
        });
    }
    assert_eq!(
        reopened
            .service
            .services_store
            .conn()
            .lock()
            .query_row("SELECT value FROM fixture_probe", [], |row| row
                .get::<_, String>(0),)
            .unwrap(),
        "committed"
    );
    reopened.service.usage_store.with_conn(|conn| {
        assert_eq!(
            conn.query_row("SELECT value FROM fixture_probe", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "committed"
        );
    });
}
