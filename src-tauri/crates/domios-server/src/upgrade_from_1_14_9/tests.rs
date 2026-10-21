use super::*;
use std::{io::Write, sync::Mutex};

const MESH_PEERS_FILE: &str = "mesh/peers.json";

#[derive(Default)]
struct MockSecretItems {
    values: Mutex<HashMap<(String, String), String>>,
}

impl MockSecretItems {
    fn insert(&self, service: &str, account: &str, value: &str) {
        self.values.lock().unwrap().insert(
            (service.to_string(), account.to_string()),
            value.to_string(),
        );
    }
}

impl SecretItems for MockSecretItems {
    fn old_service_accounts(&self) -> io::Result<Vec<(String, String)>> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .keys()
            .filter(|(service, _)| is_old_service(service))
            .cloned()
            .collect())
    }

    fn get(&self, service: &str, account: &str) -> io::Result<Option<String>> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .get(&(service.to_string(), account.to_string()))
            .cloned())
    }

    fn set(&self, service: &str, account: &str, value: &str) -> io::Result<()> {
        self.insert(service, account, value);
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> io::Result<()> {
        self.values
            .lock()
            .unwrap()
            .remove(&(service.to_string(), account.to_string()));
        Ok(())
    }
}

struct Fixture {
    _home: tempfile::TempDir,
    old: PathBuf,
    new: PathBuf,
    project: PathBuf,
    identity: ResolvedInstancePaths,
    registry: PathBuf,
}

fn fixture(storage_version: i32) -> Fixture {
    let home = tempfile::tempdir().unwrap();
    let old = home.path().join(".codeterm");
    let new = home.path().join(".domios");
    let project = home.path().join("project");
    std::fs::create_dir_all(old.join("storage")).unwrap();
    std::fs::create_dir_all(old.join("mem")).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    create_storage_schema(&old, storage_version, &project);
    create_memory_schema(&old);
    create_plan_schema(&old);
    std::fs::write(old.join("sentinel.bin"), b"one atomic move").unwrap();
    let peer = serde_json::json!({
        "peers": {
            "peer-1149": {
                "instance_id": "peer-1149",
                "instance_name": "v1.14.9 peer",
                "relay_protocol_version": 1
            }
        }
    });
    std::fs::create_dir_all(old.join("mesh")).unwrap();
    std::fs::write(
        old.join(MESH_PEERS_FILE),
        serde_json::to_vec(&peer).unwrap(),
    )
    .unwrap();
    let identity = ResolvedInstancePaths::resolve(&new, &new.join("config.toml")).unwrap();
    Fixture {
        _home: home,
        old,
        new,
        project,
        identity,
        registry: home_registry_path(),
    }
}

fn home_registry_path() -> PathBuf {
    std::env::temp_dir().join(format!("domios-upgrade-owner-{}", uuid::Uuid::new_v4()))
}

fn create_storage_schema(root: &Path, version: i32, project: &Path) {
    let connection =
        Connection::open(root.join("storage").join("state.db")).expect("create state fixture");
    connection
        .execute_batch(&format!(
            "PRAGMA application_id = {STORAGE_APPLICATION_ID}; PRAGMA user_version = {version};
             CREATE TABLE workspaces (cwd TEXT, provisioned_cwd TEXT);
             INSERT INTO workspaces (cwd, provisioned_cwd) VALUES ('{}', NULL);",
            project.display()
        ))
        .unwrap();
}

fn create_memory_schema(root: &Path) {
    let connection =
        Connection::open(root.join("mem").join("knowledge.db")).expect("create memory fixture");
    connection
        .execute_batch(
            "CREATE TABLE mem_items (
                id TEXT, uid TEXT, kind TEXT, scope TEXT, title TEXT,
                recurrence_count INTEGER, source_path TEXT, source_size INTEGER,
                source_mtime TEXT, is_pinned_wakeup INTEGER, seed_fingerprint TEXT,
                evidence_count INTEGER, false_fire_count INTEGER, promotion_threshold INTEGER
            );
            CREATE TABLE entities (id TEXT);
            CREATE TABLE relations (id INTEGER, reason TEXT);
            CREATE TABLE mem_item_history (id INTEGER);
            CREATE TABLE graph_purge_log (id INTEGER);
            CREATE TABLE lesson_audit (id INTEGER);",
        )
        .unwrap();
}

fn create_plan_schema(root: &Path) {
    let connection = Connection::open(root.join("plans.db")).expect("create plan fixture");
    connection
        .execute_batch(
            "CREATE TABLE plans (id TEXT, title TEXT, status TEXT, project_path TEXT, document_json TEXT);
            CREATE TABLE specs (id TEXT, project_path TEXT, document_json TEXT);
            CREATE TABLE plan_operations (id INTEGER, entity_id TEXT, kind TEXT, state TEXT, submitted_at INTEGER);
            CREATE TABLE legacy_imports (root TEXT, imported_at INTEGER);",
        )
        .unwrap();
}

fn init_git(root: &Path) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["init", "--quiet"])
        .output()
        .expect("git is available for fixture");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn upgrade_from_1_14_9_renames_data_and_preserves_stored_pairings_and_secret_items() {
    let fixture = fixture(STORAGE_SCHEMA_VERSION);
    init_git(&fixture.project);
    std::fs::create_dir(fixture.project.join(PROJECT_OLD_DIR)).unwrap();
    std::fs::write(
        fixture.project.join(PROJECT_OLD_DIR).join("plan.json"),
        b"untracked plan",
    )
    .unwrap();
    let secrets = MockSecretItems::default();
    secrets.insert(OLD_LOCAL_SERVICE, "api-token", "opaque-local-value");
    secrets.insert("codeterm.secrets.bitwarden", "session", "opaque-session");

    let (owner, notices) = migrate(fixture.identity, &fixture.registry, true, &secrets).unwrap();

    assert!(fixture.new.is_dir());
    assert!(!fixture.old.exists());
    assert_eq!(
        std::fs::read(fixture.new.join("sentinel.bin")).unwrap(),
        b"one atomic move"
    );
    let peers: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.new.join(MESH_PEERS_FILE)).unwrap()).unwrap();
    assert_eq!(
        peers["peers"]["peer-1149"]["instance_id"], "peer-1149",
        "the stored 1.14.9 pairing remains readable after the directory rename"
    );
    assert!(fixture
        .project
        .join(PROJECT_NEW_DIR)
        .join("plan.json")
        .exists());
    assert!(!fixture.project.join(PROJECT_OLD_DIR).exists());
    assert_eq!(
        secrets
            .get(NEW_LOCAL_SERVICE, "api-token")
            .unwrap()
            .as_deref(),
        Some("opaque-local-value")
    );
    assert_eq!(
        secrets
            .get("domios.secrets.bitwarden", "session")
            .unwrap()
            .as_deref(),
        Some("opaque-session")
    );
    assert!(secrets
        .get("codeterm.secrets.bitwarden", "session")
        .unwrap()
        .is_none());
    assert!(secrets
        .get(OLD_LOCAL_SERVICE, "api-token")
        .unwrap()
        .is_none());
    assert!(notices.is_empty());
    drop(owner);
    let _ = std::fs::remove_dir_all(&fixture.registry);
}

#[test]
fn upgrade_from_1_14_9_refuses_both_data_directories_without_merging() {
    let fixture = fixture(STORAGE_SCHEMA_VERSION);
    std::fs::create_dir_all(&fixture.new).unwrap();
    std::fs::write(fixture.new.join("new.marker"), b"new").unwrap();

    let result = migrate(
        fixture.identity,
        &fixture.registry,
        true,
        &MockSecretItems::default(),
    );
    let error = match result {
        Err(error) => error.to_string(),
        Ok((owner, _)) => {
            drop(owner);
            panic!("both data directories must refuse startup")
        }
    };

    assert!(error.contains(fixture.old.to_string_lossy().as_ref()));
    assert!(error.contains(fixture.new.to_string_lossy().as_ref()));
    assert!(fixture.old.join("sentinel.bin").exists());
    assert_eq!(
        std::fs::read(fixture.new.join("new.marker")).unwrap(),
        b"new"
    );
    let _ = std::fs::remove_dir_all(&fixture.registry);
}

#[test]
fn destination_model_accepts_only_the_native_client_startup_trail() {
    let trail_only = BTreeSet::from([
        (PathBuf::from("logs"), DestinationEntryKind::Directory),
        (
            PathBuf::from(CLIENT_STARTUP_LOG),
            DestinationEntryKind::File,
        ),
    ]);
    assert_eq!(
        classify_destination(&trail_only),
        DestinationContent::ClientStartupTrail
    );
    assert_eq!(
        classify_destination(&BTreeSet::from([(
            PathBuf::from("logs"),
            DestinationEntryKind::Directory,
        )])),
        DestinationContent::Empty
    );
    assert_eq!(
        classify_destination(&BTreeSet::new()),
        DestinationContent::Empty
    );

    let mut with_other_content = trail_only.clone();
    with_other_content.insert((PathBuf::from("config.toml"), DestinationEntryKind::File));
    assert_eq!(
        classify_destination(&with_other_content),
        DestinationContent::Other
    );
}

#[test]
fn upgrade_from_1_14_9_moves_preexisting_client_trail_before_first_server_start() {
    let fixture = fixture(STORAGE_SCHEMA_VERSION);
    let startup_log = fixture.new.join(CLIENT_STARTUP_LOG);
    std::fs::create_dir_all(startup_log.parent().unwrap()).unwrap();
    let mut client_trail = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&startup_log)
        .unwrap();
    writeln!(client_trail, "client trail before first server start").unwrap();
    drop(client_trail);

    let (owner, notices) = run(fixture.identity, &fixture.registry).unwrap();

    let relocated_log = fixture.new.join(CLIENT_STARTUP_LOG);
    let mut client_trail = std::fs::OpenOptions::new()
        .append(true)
        .open(&relocated_log)
        .unwrap();
    writeln!(client_trail, "client trail after relocation").unwrap();
    drop(client_trail);
    assert_eq!(
        std::fs::read(relocated_log).unwrap(),
        b"client trail before first server start\nclient trail after relocation\n"
    );
    assert!(fixture.new.join("sentinel.bin").exists());
    assert!(!fixture.old.exists());
    assert!(notices.is_empty());
    drop(owner);
    let _ = std::fs::remove_dir_all(&fixture.registry);
}

#[test]
fn upgrade_from_1_14_9_fresh_install_skips_project_directory_relocation() {
    let home = tempfile::tempdir().unwrap();
    let new_data = home.path().join(".domios");
    std::fs::create_dir(&new_data).unwrap();
    let identity =
        ResolvedInstancePaths::resolve(&new_data, &new_data.join("config.toml")).unwrap();
    let registry = home_registry_path();

    let (owner, notices) = run(identity, &registry).unwrap();

    assert!(notices.is_empty());
    assert!(new_data.is_dir());
    assert!(!home.path().join(".codeterm").exists());
    drop(owner);
    let _ = std::fs::remove_file(registry);
}

#[test]
fn upgrade_from_1_14_9_resumes_after_trail_staging_left_an_empty_destination() {
    let fixture = fixture(STORAGE_SCHEMA_VERSION);
    std::fs::create_dir_all(fixture.old.join("logs")).unwrap();
    std::fs::write(
        fixture.old.join(CLIENT_STARTUP_LOG),
        b"client trail staged before an interrupted root rename",
    )
    .unwrap();
    std::fs::create_dir_all(fixture.new.join("logs")).unwrap();

    let (owner, notices) = run(fixture.identity, &fixture.registry).unwrap();

    assert_eq!(
        std::fs::read(fixture.new.join(CLIENT_STARTUP_LOG)).unwrap(),
        b"client trail staged before an interrupted root rename"
    );
    assert!(fixture.new.join("sentinel.bin").exists());
    assert!(!fixture.old.exists());
    assert!(notices.is_empty());
    drop(owner);
    let _ = std::fs::remove_dir_all(&fixture.registry);
}

#[test]
fn upgrade_from_1_14_9_refuses_predecessor_storage_schema_with_required_step() {
    let fixture = fixture(STORAGE_SCHEMA_VERSION - 1);

    let result = migrate(
        fixture.identity,
        &fixture.registry,
        true,
        &MockSecretItems::default(),
    );
    let error = match result {
        Err(error) => error.to_string(),
        Ok((owner, _)) => {
            drop(owner);
            panic!("a pre-1.14.9 storage schema must refuse startup")
        }
    };

    assert!(error.contains(fixture.old.to_string_lossy().as_ref()));
    assert!(error.contains("schema storage"));
    assert!(error.contains("user_version"));
    assert!(error.contains("install 1.14.9 first"));
    assert!(fixture.old.is_dir());
    assert!(!fixture.new.exists());
    let _ = std::fs::remove_dir_all(&fixture.registry);
}

#[test]
fn upgrade_from_1_14_9_keeps_git_tracked_project_directory_and_notifies_owner() {
    let fixture = fixture(STORAGE_SCHEMA_VERSION);
    init_git(&fixture.project);
    std::fs::create_dir(fixture.project.join(PROJECT_OLD_DIR)).unwrap();
    std::fs::write(
        fixture.project.join(PROJECT_OLD_DIR).join("tracked.md"),
        b"tracked project file",
    )
    .unwrap();
    let add = Command::new("git")
        .arg("-C")
        .arg(&fixture.project)
        .args(["add", ".codeterm/tracked.md"])
        .status()
        .unwrap();
    assert!(add.success());

    let (owner, notices) = migrate(
        fixture.identity,
        &fixture.registry,
        true,
        &MockSecretItems::default(),
    )
    .unwrap();

    assert!(fixture.project.join(PROJECT_OLD_DIR).exists());
    assert!(!fixture.project.join(PROJECT_NEW_DIR).exists());
    assert!(notices.iter().any(|notice| {
        notice.contains("git mv .codeterm .domios")
            && notice.contains(&fixture.project.join(PROJECT_OLD_DIR).display().to_string())
    }));
    drop(owner);
    let _ = std::fs::remove_dir_all(&fixture.registry);
}

#[test]
fn upgrade_from_1_14_9_never_moves_project_directories_in_dev() {
    let fixture = fixture(STORAGE_SCHEMA_VERSION);
    std::fs::create_dir(fixture.project.join(PROJECT_OLD_DIR)).unwrap();
    let (owner, notices) = migrate(
        fixture.identity,
        &fixture.registry,
        false,
        &MockSecretItems::default(),
    )
    .unwrap();

    assert!(fixture.project.join(PROJECT_OLD_DIR).exists());
    assert!(!fixture.project.join(PROJECT_NEW_DIR).exists());
    assert!(notices.is_empty());
    drop(owner);
    let _ = std::fs::remove_dir_all(&fixture.registry);
}

fn workspace_cwds(root: &Path) -> Vec<String> {
    let connection = Connection::open(root.join("storage").join("state.db")).unwrap();
    let mut statement = connection
        .prepare("SELECT cwd FROM workspaces ORDER BY cwd")
        .unwrap();
    let rows = statement.query_map([], |row| row.get(0)).unwrap();
    rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
}

#[test]
fn upgrade_from_1_14_9_rebases_stored_paths_after_relocation_and_on_later_starts() {
    let fixture = fixture(STORAGE_SCHEMA_VERSION);
    let insert = |root: &Path, cwd: PathBuf| {
        Connection::open(root.join("storage").join("state.db"))
            .unwrap()
            .execute(
                "INSERT INTO workspaces (cwd, provisioned_cwd) VALUES (?1, NULL)",
                [cwd.display().to_string()],
            )
            .unwrap();
    };
    insert(&fixture.old, fixture.old.join("general"));
    let secrets = MockSecretItems::default();

    let (owner, notices) =
        migrate(fixture.identity.clone(), &fixture.registry, false, &secrets).unwrap();
    drop(owner);

    assert!(notices.is_empty(), "{notices:?}");
    let mut expected = vec![
        fixture.new.join("general").display().to_string(),
        fixture.project.display().to_string(),
    ];
    expected.sort();
    assert_eq!(workspace_cwds(&fixture.new), expected);

    insert(&fixture.new, fixture.old.join("provisioned-workspaces"));
    let (owner, notices) = migrate(fixture.identity, &fixture.registry, false, &secrets).unwrap();

    assert!(notices.is_empty(), "{notices:?}");
    expected.push(
        fixture
            .new
            .join("provisioned-workspaces")
            .display()
            .to_string(),
    );
    expected.sort();
    assert_eq!(workspace_cwds(&fixture.new), expected);
    drop(owner);
    let _ = std::fs::remove_dir_all(&fixture.registry);
}

#[test]
fn secret_file_migration_moves_namespaces_and_legacy_service_files() {
    let root = tempfile::tempdir().unwrap();
    let shared = root.path().join(".secrets");
    std::fs::write(
        &shared,
        serde_json::to_vec(&HashMap::from([
            (
                "codeterm-secrets:api-token".to_string(),
                "local".to_string(),
            ),
            (
                "codeterm.secrets.bitwarden:session".to_string(),
                "provider".to_string(),
            ),
            (
                "codeterm.plugin.bitwarden:api-key".to_string(),
                "plugin".to_string(),
            ),
        ]))
        .unwrap(),
    )
    .unwrap();
    let legacy_directory = root.path().join("keychain");
    std::fs::create_dir_all(&legacy_directory).unwrap();
    std::fs::write(
        legacy_directory.join("codeterm-secrets.json"),
        br#"{"legacy-token":"legacy"}"#,
    )
    .unwrap();
    let items = FileSecretItems::new(root.path().to_path_buf());

    migrate_secret_namespaces(&items).unwrap();

    assert_eq!(
        items
            .get(NEW_LOCAL_SERVICE, "api-token")
            .unwrap()
            .as_deref(),
        Some("local")
    );
    assert_eq!(
        items
            .get(NEW_LOCAL_SERVICE, "legacy-token")
            .unwrap()
            .as_deref(),
        Some("legacy")
    );
    assert_eq!(
        items
            .get("domios.secrets.bitwarden", "session")
            .unwrap()
            .as_deref(),
        Some("provider")
    );
    assert!(items
        .get("codeterm.secrets.bitwarden", "session")
        .unwrap()
        .is_none());
    assert_eq!(
        items
            .get("codeterm.plugin.bitwarden", "api-key")
            .unwrap()
            .as_deref(),
        Some("plugin")
    );
    assert!(items.get(OLD_LOCAL_SERVICE, "api-token").unwrap().is_none());
    assert!(!legacy_directory.join("codeterm-secrets.json").exists());
}

#[test]
fn upgrade_from_1_14_9_accepts_unlocked_provider_update_lock() {
    let fixture = fixture(STORAGE_SCHEMA_VERSION);
    std::fs::create_dir_all(&fixture.new).unwrap();
    std::fs::write(fixture.new.join("provider-update.lock"), b"obsolete pid").unwrap();
    let (owner, notices) = run(fixture.identity, &fixture.registry).unwrap();
    assert!(fixture.new.join("sentinel.bin").exists());
    assert!(!fixture.old.exists());
    assert!(!fixture.new.join("provider-update.lock").exists());
    assert!(notices.is_empty());
    drop(owner);
    let _ = std::fs::remove_dir_all(&fixture.registry);
}

#[test]
fn upgrade_from_1_14_9_refuses_locked_provider_update_lock() {
    let fixture = fixture(STORAGE_SCHEMA_VERSION);
    std::fs::create_dir_all(&fixture.new).unwrap();
    let path = fixture.new.join("provider-update.lock");
    let lock = std::fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    lock.lock().unwrap();
    let error = run(fixture.identity, &fixture.registry)
        .err()
        .expect("active lock must refuse relocation");
    assert!(error.to_string().contains("provider-update.lock"));
    assert!(fixture.old.join("sentinel.bin").exists());
    assert!(path.exists());
    drop(lock);
    let _ = std::fs::remove_dir_all(&fixture.registry);
}
