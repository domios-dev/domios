use super::*;
use codeterm_service_core::mem::{
    backend::{migrate::transfer, StoreTable},
    embedder::{ActiveEmbedder, Content, EmbedError, EmbeddingProvider, Modality},
};

mod pending_extraction;

#[test]
fn mem_backend_startup_decision_is_symmetric() {
    for selected in [MemoryBackend::Sqlite, MemoryBackend::Postgres] {
        for other in [None, Some(0), Some(1)] {
            assert!(startup_decision(selected, 1, other).is_ok());
            let result = startup_decision(selected, 0, other);
            if other == Some(1) {
                assert_eq!(
                    result,
                    Err(MemBackendError::EmptySelection {
                        selected,
                        other: selected.other()
                    })
                );
                assert!(result
                    .unwrap_err()
                    .to_string()
                    .contains(&format!("domios mem backend migrate --to {selected}")));
            } else {
                assert!(result.is_ok());
            }
        }
    }
}

#[test]
fn mem_backend_postgres_never_falls_back() {
    let dir = tempfile::tempdir().unwrap();
    for url in [None, Some(""), Some("  ")] {
        let opener = MemBackendOpener {
            data_dir: dir.path(),
            postgres_url: url,
        };
        assert!(matches!(
            opener.open(MemoryBackend::Postgres, false),
            Err(MemBackendError::MissingPostgresUrl)
        ));
    }
    #[cfg(not(feature = "mem-postgres"))]
    assert!(matches!(
        MemBackendOpener {
            data_dir: dir.path(),
            postgres_url: Some("postgresql://fixture")
        }
        .open(MemoryBackend::Postgres, false),
        Err(MemBackendError::PostgresNotBuilt)
    ));
    assert!(!dir.path().join("mem/knowledge.db").exists());
}

#[test]
fn mem_backend_reads_only_the_reserved_secret_key() {
    use secrets::SecretStore;
    let store = Arc::new(secrets::mock::MockStore::new());
    assert!(postgres_url(store.clone()).unwrap().is_none());
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(store.save(secrets::SaveRequest {
            name: POSTGRES_URL_KEY.into(),
            value: secrets::SecretString::new("postgresql://fixture".into()),
            notes: None,
            folder: None,
            organization: None,
            collection: None,
            overwrite: false,
        }))
        .unwrap();
    assert_eq!(
        postgres_url(store).unwrap().unwrap().expose_secret(),
        "postgresql://fixture"
    );
    assert_eq!(
        postgres_url(Arc::new(secrets::mock::MockStore::new_unavailable(
            "fixture"
        )))
        .unwrap_err(),
        MemBackendError::SecretUnavailable
    );
}

#[test]
fn mem_backend_migration_lock_refuses_a_running_daemon() {
    assert!(matches!(
        migrate::ownership_decision(std::io::Error::from(std::io::ErrorKind::AlreadyExists)),
        migrate::MigrationError::DaemonRunning
    ));
    assert!(matches!(
        migrate::ownership_decision(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        migrate::MigrationError::Failed(_)
    ));
    let dir = tempfile::tempdir().unwrap();
    let identity = codeterm_service_core::ResolvedInstancePaths::resolve(
        &dir.path().join("data"),
        &dir.path().join("config.toml"),
    )
    .unwrap();
    let registry = dir.path().join("owners");
    let _owner =
        codeterm_service_core::PathOwnership::try_acquire(identity.clone(), &registry).unwrap();
    let refused = codeterm_service_core::PathOwnership::try_acquire(identity, &registry)
        .err()
        .unwrap();
    assert!(matches!(
        migrate::ownership_decision(refused),
        migrate::MigrationError::DaemonRunning
    ));
}

#[test]
fn mem_backend_migration_parser_rejects_ambiguity() {
    let args = |args: &[&str]| args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
    assert_eq!(
        migrate::parse(&args(&["--to", "postgres", "--json"])).unwrap(),
        migrate::Options {
            to: MemoryBackend::Postgres,
            into_nonempty: false,
            json: true
        }
    );
    for input in [
        &[][..],
        &["--to"][..],
        &["--to", "unknown"][..],
        &["--to", "sqlite", "--to", "postgres"][..],
    ] {
        assert!(migrate::parse(&args(input)).is_err());
    }
}

struct Provider;
impl EmbeddingProvider for Provider {
    fn id(&self) -> &str {
        "fixture"
    }
    fn model(&self) -> &str {
        "migration"
    }
    fn dimension(&self) -> usize {
        3
    }
    fn max_tokens(&self) -> usize {
        512
    }
    fn modalities(&self) -> &[Modality] {
        &[Modality::Text]
    }
    fn remote(&self) -> bool {
        false
    }
    fn embed_passage(&self, _: &Content) -> Result<Vec<f32>, EmbedError> {
        Ok(vec![1.0, 0.0, 0.0])
    }
    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        self.embed_passage(content)
    }
}

#[test]
fn mem_backend_transfer_reindexes_only_the_target_through_shadow() {
    use codeterm_service_core::mem::backend::WriteContext;
    let dir = tempfile::tempdir().unwrap();
    let source: Arc<dyn MemStore> =
        Arc::new(SqliteMemStore::open_in_memory(&dir.path().join("source")).unwrap());
    let target: Arc<dyn MemStore> =
        Arc::new(SqliteMemStore::open_in_memory(&dir.path().join("target")).unwrap());
    let ctx = WriteContext::system("fixture");
    let item = codeterm_mem::NewMemItem {
        kind: codeterm_mem::MemKind::Fact,
        scope: codeterm_mem::MemScope::Global,
        title: "migration".into(),
        summary: None,
        area: None,
        tags: vec![],
        body: "migration fixture text".into(),
        source: None,
        importance: 2.0,
        source_path: None,
        pinned: false,
    };
    let id = source.save(&ctx, &item, false).unwrap().id;
    let before = source.table_counts().unwrap();
    let mut expected = transfer(&source, &target, false).unwrap();
    let embedder = ActiveEmbedder::new(Arc::new(Provider));
    migrate::reindex(&target, embedder.clone(), &mut expected).unwrap();
    codeterm_mem::backend::migrate::verify_counts(&expected, &target.table_counts().unwrap())
        .unwrap();
    assert!(expected[&StoreTable::Vectors] > 0);
    assert_eq!(source.table_counts().unwrap(), before);
    assert_eq!(
        target.get(&id).unwrap().unwrap().body,
        source.get(&id).unwrap().unwrap().body
    );
    assert_eq!(target.vector_stamp().unwrap(), Some(embedder.identity()));
}

#[test]
fn mem_backend_sqlite_import_target_does_not_seed_or_clean_source_records() {
    let dir = tempfile::tempdir().unwrap();
    let opener = MemBackendOpener {
        data_dir: dir.path(),
        postgres_url: None,
    };
    let target = opener.migration_target(MemoryBackend::Sqlite).unwrap();
    let counts = target.table_counts().unwrap();
    assert_eq!(counts.len(), 21);
    assert!(counts.values().all(|count| *count == 0));
}
