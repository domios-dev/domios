use codeterm_service_core::{PathOwnership, ResolvedInstancePaths};
use rusqlite::{Connection, OpenFlags};
mod relocation_holders;
mod stored_paths;
use relocation_holders::{rename_legacy_root, SystemLegacyRootHolderProbe};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    process::Command,
};
use stored_paths::{rebase_stored_paths, PathRebase};

const STORAGE_APPLICATION_ID: i32 = 0x4354_5354;
const STORAGE_SCHEMA_VERSION: i32 = 17;
const OLD_LOCAL_SERVICE: &str = "codeterm-secrets";
const NEW_LOCAL_SERVICE: &str = "domios-secrets";
const OLD_PROVIDER_SERVICE_PREFIX: &str = "codeterm.secrets.";
const NEW_PROVIDER_SERVICE_PREFIX: &str = "domios.secrets.";
const PROJECT_OLD_DIR: &str = ".codeterm";
const PROJECT_NEW_DIR: &str = ".domios";
const CLIENT_STARTUP_LOG: &str = "logs/client-startup.log";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum DestinationEntryKind {
    Directory,
    File,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DestinationContent {
    Empty,
    ClientStartupTrail,
    Other,
}

pub(super) fn run(
    identity: ResolvedInstancePaths,
    ownership_registry: &Path,
) -> io::Result<(PathOwnership, Vec<String>)> {
    let secret_items = FileSecretItems::new(identity.data().to_path_buf());
    migrate(
        identity,
        ownership_registry,
        !cfg!(debug_assertions),
        &secret_items,
    )
}

fn migrate(
    identity: ResolvedInstancePaths,
    ownership_registry: &Path,
    migrate_projects: bool,
    secret_items: &dyn SecretItems,
) -> io::Result<(PathOwnership, Vec<String>)> {
    let owner = PathOwnership::try_acquire(identity.clone(), ownership_registry)?;
    let new_data = identity.data();
    let old_data = legacy_data_path(new_data);
    let new_exists = directory_exists(new_data)?;
    let old_exists = old_data
        .as_deref()
        .map(directory_exists)
        .transpose()?
        .unwrap_or(false);

    let stray_lock = if old_exists && new_exists {
        acquire_stray_provider_lock(new_data)?
    } else {
        None
    };
    let destination_content = if old_exists && new_exists {
        let mut inventory = destination_inventory(new_data).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "Cannot relocate data: both {} and {} exist; cannot inspect destination contents: {error}",
                    old_data.as_deref().unwrap().display(),
                    new_data.display()
                ),
            )
        })?;
        if stray_lock.is_some() {
            inventory.remove(&(
                PathBuf::from("provider-update.lock"),
                DestinationEntryKind::File,
            ));
        }
        let content = classify_destination(&inventory);
        if content == DestinationContent::Other {
            return Err(io::Error::other(format!(
                "Cannot relocate data: both {} and {} exist; resolve them separately before starting Domios",
                old_data.as_deref().unwrap().display(),
                new_data.display()
            )));
        }
        Some(content)
    } else {
        None
    };

    let mut notices = Vec::new();
    let mut project_roots = Vec::new();
    if old_exists {
        let old_data = old_data.as_deref().expect("old directory exists");
        let old_identity = legacy_identity(&identity, old_data)?;
        let old_owner =
            PathOwnership::try_acquire(old_identity, ownership_registry).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!(
                        "Cannot relocate {} while its previous instance owns the directory: {error}",
                        old_data.display()
                    ),
                )
            })?;
        project_roots = validate_1_14_9_schema(old_data)?;
        if old_data.parent() != new_data.parent() {
            return Err(io::Error::other(format!(
                "Cannot atomically relocate {} to {} because they are not sibling directories on the same volume",
                old_data.display(),
                new_data.display()
            )));
        }
        if stray_lock.is_some() {
            std::fs::remove_file(new_data.join("provider-update.lock"))?;
        }
        drop(stray_lock);
        let staged_client_trail = match destination_content {
            Some(DestinationContent::ClientStartupTrail) => {
                Some(stage_client_startup_trail(new_data, old_data)?)
            }
            Some(DestinationContent::Empty) => {
                remove_empty_destination(new_data, old_data)?;
                None
            }
            Some(DestinationContent::Other) | None => None,
        };
        if let Err(error) = rename_legacy_root(old_data, new_data, &SystemLegacyRootHolderProbe) {
            let rollback = staged_client_trail
                .as_deref()
                .map(|staged| restore_client_startup_trail(staged, new_data));
            let reason = match rollback {
                Some(Err(rollback_error)) => {
                    format!("{error}; could not restore the client startup trail: {rollback_error}")
                }
                _ => error.to_string(),
            };
            return Err(io::Error::new(error.kind(), reason));
        }
        drop(old_owner);
    }
    if let Some(rebase) = old_data
        .as_deref()
        .and_then(|old_data| PathRebase::new(old_data, new_data))
    {
        notices.extend(rebase_stored_paths(new_data, &rebase));
        for root in &mut project_roots {
            *root = rebase.rebase_path(root);
        }
    }

    migrate_secret_namespaces(secret_items)?;
    notices.extend(migrate_project_directories(
        &project_roots,
        migrate_projects,
    ));
    Ok((owner, notices))
}

fn legacy_data_path(new_data: &Path) -> Option<PathBuf> {
    let name = new_data.file_name()?.to_str()?;
    let old_name = if name == ".domios" {
        ".codeterm".to_string()
    } else if let Some(suffix) = name.strip_prefix(".domios-dev") {
        format!(".codeterm-dev{suffix}")
    } else {
        return None;
    };
    Some(new_data.with_file_name(old_name))
}

fn legacy_identity(
    identity: &ResolvedInstancePaths,
    old_data: &Path,
) -> io::Result<ResolvedInstancePaths> {
    let old_config = identity
        .config()
        .strip_prefix(identity.data())
        .map(|relative| old_data.join(relative))
        .unwrap_or_else(|_| identity.config().to_path_buf());
    ResolvedInstancePaths::resolve(old_data, &old_config)
}

fn directory_exists(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(true),
        Ok(_) => Err(io::Error::other(format!(
            "Runtime identity path is not a directory: {}",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn destination_inventory(root: &Path) -> io::Result<BTreeSet<(PathBuf, DestinationEntryKind)>> {
    let mut inventory = BTreeSet::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let path = entry.path();
        let relative = PathBuf::from(&name);
        let metadata = std::fs::symlink_metadata(&path)?;
        let kind = entry_kind(&metadata);
        inventory.insert((relative.clone(), kind));
        if relative == Path::new("logs") && kind == DestinationEntryKind::Directory {
            for log_entry in std::fs::read_dir(path)? {
                let log_entry = log_entry?;
                let log_name = log_entry.file_name();
                let log_path = log_entry.path();
                let log_metadata = std::fs::symlink_metadata(&log_path)?;
                inventory.insert((relative.join(log_name), entry_kind(&log_metadata)));
            }
        }
    }
    Ok(inventory)
}

fn entry_kind(metadata: &std::fs::Metadata) -> DestinationEntryKind {
    if metadata.file_type().is_dir() {
        DestinationEntryKind::Directory
    } else if metadata.file_type().is_file() {
        DestinationEntryKind::File
    } else {
        DestinationEntryKind::Other
    }
}

fn acquire_stray_provider_lock(root: &Path) -> io::Result<Option<std::fs::File>> {
    let path = root.join("provider-update.lock");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() {
        return Ok(None);
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)?;
    file.try_lock().map_err(|error| {
        io::Error::other(format!(
            "Cannot relocate data while {} is locked: {error}",
            path.display()
        ))
    })?;
    Ok(Some(file))
}

fn classify_destination(
    inventory: &BTreeSet<(PathBuf, DestinationEntryKind)>,
) -> DestinationContent {
    if inventory.len() == 2
        && inventory.contains(&(PathBuf::from("logs"), DestinationEntryKind::Directory))
        && inventory.contains(&(
            PathBuf::from(CLIENT_STARTUP_LOG),
            DestinationEntryKind::File,
        ))
    {
        DestinationContent::ClientStartupTrail
    } else if inventory.is_empty()
        || (inventory.len() == 1
            && inventory.contains(&(PathBuf::from("logs"), DestinationEntryKind::Directory)))
    {
        DestinationContent::Empty
    } else {
        DestinationContent::Other
    }
}

fn stage_client_startup_trail(new_data: &Path, old_data: &Path) -> io::Result<PathBuf> {
    let source = new_data.join(CLIENT_STARTUP_LOG);
    let old_logs = old_data.join("logs");
    std::fs::create_dir_all(&old_logs)?;
    let primary = old_logs.join("client-startup.log");
    let destination = if path_exists(&primary)? {
        unique_client_trail_path(&old_logs)?
    } else {
        primary
    };
    std::fs::rename(&source, &destination).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "Cannot move client startup trail from {} into {}: {error}",
                source.display(),
                destination.display()
            ),
        )
    })?;
    let cleanup =
        std::fs::remove_dir(new_data.join("logs")).and_then(|()| std::fs::remove_dir(new_data));
    if let Err(error) = cleanup {
        let rollback = restore_client_startup_trail(&destination, new_data);
        let reason = match rollback {
            Ok(()) => error.to_string(),
            Err(rollback_error) => {
                format!("{error}; could not restore the client startup trail: {rollback_error}")
            }
        };
        return Err(io::Error::new(error.kind(), reason));
    }
    Ok(destination)
}

fn remove_empty_destination(new_data: &Path, old_data: &Path) -> io::Result<()> {
    let logs = new_data.join("logs");
    match std::fs::remove_dir(&logs) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!(
                    "Cannot remove empty destination {} before relocating {}: {error}",
                    new_data.display(),
                    old_data.display()
                ),
            ));
        }
    }
    std::fs::remove_dir(new_data).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "Cannot remove empty destination {} before relocating {}: {error}",
                new_data.display(),
                old_data.display()
            ),
        )
    })
}

fn restore_client_startup_trail(staged: &Path, new_data: &Path) -> io::Result<()> {
    let destination = new_data.join(CLIENT_STARTUP_LOG);
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("Client startup trail has no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    std::fs::rename(staged, destination)
}

fn unique_client_trail_path(directory: &Path) -> io::Result<PathBuf> {
    for _ in 0..8 {
        let candidate = directory.join(format!(
            "client-startup.log.relocated-{}",
            uuid::Uuid::new_v4()
        ));
        if !path_exists(&candidate)? {
            return Ok(candidate);
        }
    }
    Err(io::Error::other(format!(
        "Cannot reserve a path for the client startup trail in {}",
        directory.display()
    )))
}

fn path_exists(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn schema_refusal(root: &Path, schema: &str, detail: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!(
        "Cannot relocate {}: schema {schema} is not the 1.14.9 schema ({detail}); install 1.14.9 first",
        root.display()
    ))
}

fn open_read_only(root: &Path, schema: &str, path: &Path) -> io::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| schema_refusal(root, schema, format!("{}: {error}", path.display())))
}

fn pragma_version(root: &Path, schema: &str, connection: &Connection) -> io::Result<i32> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| schema_refusal(root, schema, error))
}

fn table_columns(
    root: &Path,
    schema: &str,
    connection: &Connection,
    table: &str,
) -> io::Result<BTreeSet<String>> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| schema_refusal(root, schema, error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| schema_refusal(root, schema, error))?;
    rows.collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| schema_refusal(root, schema, error))
}

fn require_columns(
    root: &Path,
    schema: &str,
    connection: &Connection,
    table: &str,
    required: &[&str],
) -> io::Result<()> {
    let actual = table_columns(root, schema, connection, table)?;
    let missing: Vec<_> = required
        .iter()
        .copied()
        .filter(|column| !actual.contains(*column))
        .collect();
    if !missing.is_empty() {
        return Err(schema_refusal(
            root,
            schema,
            format!("{table} is missing columns {}", missing.join(", ")),
        ));
    }
    Ok(())
}

fn require_table(
    root: &Path,
    schema: &str,
    connection: &Connection,
    table: &str,
) -> io::Result<()> {
    if table_columns(root, schema, connection, table)?.is_empty() {
        return Err(schema_refusal(
            root,
            schema,
            format!("table {table} is missing"),
        ));
    }
    Ok(())
}

fn validate_1_14_9_schema(root: &Path) -> io::Result<Vec<PathBuf>> {
    let state_path = root.join("storage").join("state.db");
    let state = open_read_only(root, "storage", &state_path)?;
    let app_id: i32 = state
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .map_err(|error| schema_refusal(root, "storage", error))?;
    let version = pragma_version(root, "storage", &state)?;
    if app_id != STORAGE_APPLICATION_ID || version != STORAGE_SCHEMA_VERSION {
        return Err(schema_refusal(
            root,
            "storage",
            format!(
                "application_id={app_id:#x}, user_version={version}, expected application_id={STORAGE_APPLICATION_ID:#x}, user_version={STORAGE_SCHEMA_VERSION}"
            ),
        ));
    }
    require_columns(
        root,
        "storage",
        &state,
        "workspaces",
        &["cwd", "provisioned_cwd"],
    )?;
    let project_roots = workspace_roots_from(&state, root)?;
    drop(state);

    let memory_path = root.join("mem").join("knowledge.db");
    let memory = open_read_only(root, "mem", &memory_path)?;
    let memory_version = pragma_version(root, "mem", &memory)?;
    if memory_version != 0 {
        return Err(schema_refusal(
            root,
            "mem",
            format!("user_version={memory_version}, expected the 1.14.9 fingerprint"),
        ));
    }
    require_columns(
        root,
        "mem",
        &memory,
        "mem_items",
        &[
            "id",
            "uid",
            "kind",
            "scope",
            "title",
            "recurrence_count",
            "source_path",
            "source_size",
            "source_mtime",
            "is_pinned_wakeup",
            "seed_fingerprint",
            "evidence_count",
            "false_fire_count",
            "promotion_threshold",
        ],
    )?;
    for table in [
        "entities",
        "relations",
        "mem_item_history",
        "graph_purge_log",
        "lesson_audit",
    ] {
        require_table(root, "mem", &memory, table)?;
    }
    require_columns(root, "mem", &memory, "relations", &["reason"])?;
    drop(memory);

    let plan_path = root.join("plans.db");
    let plans = open_read_only(root, "plan", &plan_path)?;
    let plan_version = pragma_version(root, "plan", &plans)?;
    if plan_version != 0 {
        return Err(schema_refusal(
            root,
            "plan",
            format!("user_version={plan_version}, expected the 1.14.9 fingerprint"),
        ));
    }
    for (table, columns) in [
        (
            "plans",
            &["id", "title", "status", "project_path", "document_json"][..],
        ),
        ("specs", &["id", "project_path", "document_json"][..]),
        (
            "plan_operations",
            &["id", "entity_id", "kind", "state", "submitted_at"][..],
        ),
        ("legacy_imports", &["root", "imported_at"][..]),
    ] {
        require_columns(root, "plan", &plans, table, columns)?;
    }
    drop(plans);
    Ok(project_roots)
}

fn workspace_roots_from(connection: &Connection, root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut statement = connection
        .prepare("SELECT cwd, provisioned_cwd FROM workspaces")
        .map_err(|error| schema_refusal(root, "storage", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })
        .map_err(|error| schema_refusal(root, "storage", error))?;
    let mut roots = BTreeSet::new();
    for row in rows {
        let (cwd, provisioned_cwd) = row.map_err(|error| schema_refusal(root, "storage", error))?;
        for candidate in [cwd, provisioned_cwd].into_iter().flatten() {
            let path = PathBuf::from(candidate);
            if path.is_absolute() {
                roots.insert(path);
            }
        }
    }
    Ok(roots.into_iter().collect())
}

fn migrate_project_directories(roots: &[PathBuf], enabled: bool) -> Vec<String> {
    if !enabled {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let mut notices = Vec::new();
    for root in roots {
        if !seen.insert(root.clone()) {
            continue;
        }
        let old = root.join(PROJECT_OLD_DIR);
        let new = root.join(PROJECT_NEW_DIR);
        if !old.is_dir() {
            continue;
        }
        if new.exists() {
            notices.push(format!(
                "Both {} and {} exist. Resolve the project directories separately; use git mv .codeterm .domios after resolving the destination.",
                old.display(),
                new.display()
            ));
            continue;
        }
        match tracked_project_files(root) {
            Ok(true) => notices.push(format!(
                "Tracked files remain in {}. Move them with git mv .codeterm .domios.",
                old.display()
            )),
            Ok(false) => match std::fs::rename(&old, &new) {
                Ok(()) => log::info!(
                    "[upgrade] moved untracked project directory {} to {}",
                    old.display(),
                    new.display()
                ),
                Err(error) => notices.push(format!(
                    "Could not rename {} to {}: {error}. Move it with git mv .codeterm .domios.",
                    old.display(),
                    new.display()
                )),
            },
            Err(error) => notices.push(format!(
                "Could not verify tracked files under {}: {error}. Leave it in place and move it with git mv .codeterm .domios.",
                old.display()
            )),
        }
    }
    notices
}

fn tracked_project_files(root: &Path) -> io::Result<bool> {
    let top = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !top.status.success() {
        return Err(io::Error::other(
            "project root is not a readable git worktree",
        ));
    }
    let top = PathBuf::from(String::from_utf8_lossy(&top.stdout).trim());
    let relative_root = root
        .strip_prefix(&top)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let old = relative_root.join(PROJECT_OLD_DIR);
    let pathspec = old.to_string_lossy().replace('\\', "/");
    let output = Command::new("git")
        .arg("-C")
        .arg(&top)
        .args(["ls-files", "--", &pathspec])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(!output.stdout.is_empty())
}

trait SecretItems {
    fn old_service_accounts(&self) -> io::Result<Vec<(String, String)>>;
    fn get(&self, service: &str, account: &str) -> io::Result<Option<String>>;
    fn set(&self, service: &str, account: &str, value: &str) -> io::Result<()>;
    fn delete(&self, service: &str, account: &str) -> io::Result<()>;
}

fn migrate_secret_namespaces(items: &dyn SecretItems) -> io::Result<()> {
    let accounts = items.old_service_accounts()?;
    for (old_service, account) in accounts {
        let new_service = new_service_name(&old_service).ok_or_else(|| {
            io::Error::other(format!("Unexpected legacy secret service {old_service}"))
        })?;
        let value = items
            .get(&old_service, &account)?
            .ok_or_else(|| io::Error::other("Legacy secret disappeared during migration"))?;
        match items.get(&new_service, &account)? {
            Some(existing) if existing != value => {
                return Err(io::Error::other(format!(
                    "Cannot move secret {old_service}:{account}: {new_service}:{account} already contains a different value"
                )))
            }
            Some(_) => {}
            None => items.set(&new_service, &account, &value)?,
        }
        items.delete(&old_service, &account)?;
    }
    Ok(())
}

fn new_service_name(old: &str) -> Option<String> {
    if old == OLD_LOCAL_SERVICE {
        Some(NEW_LOCAL_SERVICE.into())
    } else {
        old.strip_prefix(OLD_PROVIDER_SERVICE_PREFIX)
            .map(|suffix| format!("{NEW_PROVIDER_SERVICE_PREFIX}{suffix}"))
    }
}

struct FileSecretItems {
    root: PathBuf,
}

impl FileSecretItems {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn shared_path(&self) -> PathBuf {
        self.root.join(".secrets")
    }

    fn read_shared(&self) -> io::Result<HashMap<String, String>> {
        read_json_map(&self.shared_path())
    }

    fn service_path(&self, service: &str) -> PathBuf {
        self.root
            .join("keychain")
            .join(format!("{}.json", sanitize_service(service)))
    }

    fn read_legacy_service(&self, service: &str) -> io::Result<HashMap<String, String>> {
        read_json_map(&self.service_path(service))
    }

    fn write_shared(&self, values: &HashMap<String, String>) -> io::Result<()> {
        write_json_map_atomically(&self.shared_path(), values)
    }
}

impl SecretItems for FileSecretItems {
    fn old_service_accounts(&self) -> io::Result<Vec<(String, String)>> {
        let mut entries = BTreeSet::new();
        for namespaced in self.read_shared()?.keys() {
            if let Some((service, account)) = namespaced.split_once(':') {
                if is_old_service(service) {
                    entries.insert((service.to_string(), account.to_string()));
                }
            }
        }
        let directory = self.root.join("keychain");
        match std::fs::read_dir(&directory) {
            Ok(files) => {
                for file in files {
                    let file = file?;
                    let path = file.path();
                    if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                        continue;
                    }
                    let Some(service) = path.file_stem().and_then(|name| name.to_str()) else {
                        continue;
                    };
                    if !is_old_service(service) {
                        continue;
                    }
                    for account in read_json_map(&path)?.keys() {
                        entries.insert((service.to_string(), account.to_string()));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        Ok(entries.into_iter().collect())
    }

    fn get(&self, service: &str, account: &str) -> io::Result<Option<String>> {
        let shared = self
            .read_shared()?
            .get(&storage_account(service, account))
            .cloned();
        let legacy = self.read_legacy_service(service)?.get(account).cloned();
        match (shared, legacy) {
            (Some(shared), Some(legacy)) if shared != legacy => Err(io::Error::other(format!(
                "Conflicting values for secret {service}:{account} in shared and legacy files"
            ))),
            (Some(value), _) | (_, Some(value)) => Ok(Some(value)),
            (None, None) => Ok(None),
        }
    }

    fn set(&self, service: &str, account: &str, value: &str) -> io::Result<()> {
        let mut shared = self.read_shared()?;
        shared.insert(storage_account(service, account), value.to_string());
        self.write_shared(&shared)
    }

    fn delete(&self, service: &str, account: &str) -> io::Result<()> {
        let mut shared = self.read_shared()?;
        if shared.remove(&storage_account(service, account)).is_some() {
            self.write_shared(&shared)?;
        }
        let legacy_path = self.service_path(service);
        let mut legacy = self.read_legacy_service(service)?;
        if legacy.remove(account).is_some() {
            if legacy.is_empty() {
                std::fs::remove_file(legacy_path)?;
            } else {
                write_json_map_atomically(&legacy_path, &legacy)?;
            }
        }
        Ok(())
    }
}

fn is_old_service(service: &str) -> bool {
    service == OLD_LOCAL_SERVICE || service.starts_with(OLD_PROVIDER_SERVICE_PREFIX)
}

fn storage_account(service: &str, account: &str) -> String {
    format!("{}:{account}", sanitize_service(service))
}

fn sanitize_service(service: &str) -> String {
    service
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            other => other,
        })
        .collect()
}

fn read_json_map(path: &Path) -> io::Result<HashMap<String, String>> {
    match std::fs::read_to_string(path) {
        Ok(raw) if raw.trim().is_empty() => Ok(HashMap::new()),
        Ok(raw) => serde_json::from_str(&raw).map_err(|error| {
            io::Error::other(format!(
                "Cannot parse secret file {}: {error}",
                path.display()
            ))
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(error) => Err(error),
    }
}

fn write_json_map_atomically(path: &Path, values: &HashMap<String, String>) -> io::Result<()> {
    let body = serde_json::to_vec(values).map_err(io::Error::other)?;
    write_file_atomically(path, &body)
}

fn write_file_atomically(path: &Path, body: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Migrated file has no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let temp = path.with_file_name(format!(
        ".{}.migrate-{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("migrated"),
        std::process::id()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    use std::io::Write;
    if let Err(error) = file.write_all(body).and_then(|()| file.sync_all()) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

#[cfg(test)]
mod tests;
