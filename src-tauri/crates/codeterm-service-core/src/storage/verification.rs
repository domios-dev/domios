use crate::cron::clock::system_clock;
use crate::cron::store::{CronJob, CronRunInfo, CronRunStatus, CronStorageOptions, CronStore};
use crate::sentinel::store::{Sentinel, Store as SentinelStore};
use crate::session::store::{legacy_decode, SessionSnapshot, StorageDeskAdapter};
use crate::storage::migration::{import_copied_root, LegacyCodecs};
use crate::storage::{
    Captured, ComparisonReport, CronImport, CronJobImport, CronRunRecord, CronRunState,
    DomainComparison, DomainImport, ImportDomain, ImportOutcome, ImportPlan, ImportProvenance,
    ImportRefusal, ImportRequest, LegacyDecoder, SourceCapture, Storage, StorageConfig,
};
use codeterm_storage::{capture_sources, DeskAgentSessionIdentity, Storage as StorageOwner};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(30);
const MAX_DIFFS: usize = 64;
const COPY_PROCEDURE: &str = "Copy only files enumerated by this build's importer inventory. Exclude secrets/, mesh keys, and config tokens. Create a SHA-256 manifest before transfer and verify it again after copying. This verifier records per-file SHA-256 hashes before and after import and requires them to match.";

#[derive(Debug, Clone, Serialize)]
pub struct VerificationReport {
    pub schema_version: u32,
    pub passed: bool,
    pub copy_procedure: &'static str,
    pub originals_unchanged: bool,
    pub hashes_before: HashManifest,
    pub hashes_after: HashManifest,
    pub import: ImportVerification,
    pub readback: AdapterVerification,
    pub reopen: ReopenVerification,
    pub tamper: TamperVerification,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HashManifest {
    pub sha256: String,
    pub files: Vec<HashEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HashEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportVerification {
    pub outcome: &'static str,
    pub revision: String,
    pub manifest_sha256: String,
    pub capture_sha256: String,
    pub capture_matches_plan: bool,
    pub marker_matches_receipt: bool,
    pub comparison_clean: bool,
    pub archive_sealed: bool,
    pub domains: Vec<DomainVerification>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DomainVerification {
    pub domain: String,
    pub expected_entities: u64,
    pub committed_entities: u64,
    pub adapter_entities: u64,
    pub expected_items: u64,
    pub committed_items: u64,
    pub adapter_items: u64,
    pub importer_diffs: Vec<EntityDiff>,
    pub importer_diffs_elided: u64,
    pub adapter_diffs: Vec<EntityDiff>,
    pub adapter_diffs_elided: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntityDiff {
    pub subject: String,
    pub field: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdapterVerification {
    pub projection_sha256: String,
    pub domains: Vec<DomainVerification>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReopenVerification {
    pub outcome: &'static str,
    pub marker_identical: bool,
    pub projection_identical: bool,
    pub initial_projection_sha256: String,
    pub reopened_projection_sha256: String,
    pub domains: Vec<DomainVerification>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TamperVerification {
    pub archive_tamper_rejected: bool,
    pub marker_absent: bool,
    pub originals_unchanged: bool,
}

struct Readback {
    projection_sha256: String,
    domains: Vec<DomainVerification>,
}

struct DiffCollector {
    diffs: Vec<EntityDiff>,
    elided: u64,
}

impl DiffCollector {
    fn new() -> Self {
        Self {
            diffs: Vec::new(),
            elided: 0,
        }
    }

    fn push(&mut self, subject: impl Into<String>, field: impl Into<String>) {
        if self.diffs.len() < MAX_DIFFS {
            self.diffs.push(EntityDiff {
                subject: subject.into(),
                field: field.into(),
            });
        } else {
            self.elided = self.elided.saturating_add(1);
        }
    }
}

pub fn verify_copied_state(
    copy_root: &Path,
    work_root: &Path,
) -> Result<VerificationReport, String> {
    let (copy_root, work_root) = checked_roots(copy_root, work_root)?;
    if work_root.exists()
        && fs::read_dir(&work_root)
            .map_err(|error| format!("inspect work root: {error}"))?
            .next()
            .is_some()
    {
        return Err("work root must be empty".to_string());
    }
    fs::create_dir_all(&work_root).map_err(|error| format!("create work root: {error}"))?;
    let request = ImportRequest::new(copy_root.clone(), work_root.join("import-archive"));
    let capture = capture_sources(&copy_root, &request.backup_dir, request.limits)
        .map_err(|error| format!("capture importer inventory: {error}"))?;
    let hashes_before = hash_inventory(&copy_root, &capture)?;
    let expected = ImportPlan::resolve(capture, &LegacyCodecs, &BTreeMap::new())
        .map_err(|error| format!("resolve importer inventory: {error}"))?;

    let instance_root = work_root.join("instance");
    let storage = Storage::open(&instance_root, StorageConfig::default())
        .map_err(|error| format!("open verification storage: {error}"))?;
    if storage
        .legacy_import_marker()
        .wait(DEADLINE)
        .map_err(|error| format!("read initial import marker: {error}"))?
        .value
        .is_some()
    {
        return Err("verification storage already has an import marker".to_string());
    }
    let first = match import_copied_root(&storage, &request)
        .map_err(|error| format!("import copied root: {error}"))?
    {
        ImportOutcome::Committed(receipt) => *receipt,
        ImportOutcome::AlreadyCommitted(_) => {
            return Err("initial import unexpectedly found a committed marker".to_string())
        }
    };
    let comparison = first
        .comparison
        .as_ref()
        .map_err(|error| format!("importer comparison failed: {error}"))?;
    let marker_before = storage
        .legacy_import_marker()
        .wait(DEADLINE)
        .map_err(|error| format!("read committed import marker: {error}"))?
        .value
        .ok_or_else(|| "import did not publish a completion marker".to_string())?;
    let capture_matches_plan = first.capture_sha256 == expected.capture().digest();
    let marker_matches_receipt = marker_before.manifest_sha256 == first.manifest_sha256
        && marker_before.revision == first.revision;
    let initial_readback = read_adapters(
        &storage,
        &work_root.join("cron-logs"),
        &expected,
        comparison,
    )?;
    storage
        .shutdown()
        .wait(DEADLINE)
        .map_err(|error| format!("shutdown initial storage: {error}"))?;
    drop(storage);

    let reopened = Storage::open(&instance_root, StorageConfig::default())
        .map_err(|error| format!("reopen verification storage: {error}"))?;
    let reopen_outcome = import_copied_root(&reopened, &request)
        .map_err(|error| format!("verify idempotent reopen: {error}"))?;
    let (reopen_name, marker_identical) = match reopen_outcome {
        ImportOutcome::AlreadyCommitted(marker) => ("AlreadyCommitted", marker == marker_before),
        ImportOutcome::Committed(_) => ("UnexpectedCommit", false),
    };
    let reopened_readback = read_adapters(
        &reopened,
        &work_root.join("cron-logs"),
        &expected,
        comparison,
    )?;
    let marker_after = reopened
        .legacy_import_marker()
        .wait(DEADLINE)
        .map_err(|error| format!("read reopened import marker: {error}"))?
        .value;
    let marker_identical = marker_identical && marker_after == Some(marker_before);
    reopened
        .shutdown()
        .wait(DEADLINE)
        .map_err(|error| format!("shutdown reopened storage: {error}"))?;
    drop(reopened);

    let tamper = run_tamper_leg(&copy_root, &work_root, &hashes_before)?;
    let after_capture =
        capture_sources(&copy_root, &work_root.join("after-capture"), request.limits)
            .map_err(|error| format!("capture originals after verification: {error}"))?;
    let hashes_after = hash_inventory(&copy_root, &after_capture)?;
    let originals_unchanged = hashes_before == hashes_after;
    let comparison_clean = comparison.is_clean();
    let archive_sealed = first.archive.is_ok();
    let reopen_domains_clean = readback_clean(&reopened_readback);
    let initial_domains_clean = readback_clean(&initial_readback);
    let projection_identical =
        initial_readback.projection_sha256 == reopened_readback.projection_sha256;
    let tamper_clean =
        tamper.archive_tamper_rejected && tamper.marker_absent && tamper.originals_unchanged;
    let passed = comparison_clean
        && archive_sealed
        && capture_matches_plan
        && marker_matches_receipt
        && initial_domains_clean
        && reopen_domains_clean
        && marker_identical
        && projection_identical
        && originals_unchanged
        && tamper_clean;
    let import = ImportVerification {
        outcome: "Committed",
        revision: format!("{:?}", first.revision),
        manifest_sha256: first.manifest_sha256.to_hex(),
        capture_sha256: first.capture_sha256.to_hex(),
        capture_matches_plan,
        marker_matches_receipt,
        comparison_clean,
        archive_sealed,
        domains: initial_readback.domains.clone(),
    };
    Ok(VerificationReport {
        schema_version: 1,
        passed,
        copy_procedure: COPY_PROCEDURE,
        originals_unchanged,
        hashes_before,
        hashes_after,
        import,
        readback: AdapterVerification {
            projection_sha256: initial_readback.projection_sha256.clone(),
            domains: initial_readback.domains,
        },
        reopen: ReopenVerification {
            outcome: reopen_name,
            marker_identical,
            projection_identical,
            initial_projection_sha256: initial_readback.projection_sha256,
            reopened_projection_sha256: reopened_readback.projection_sha256,
            domains: reopened_readback.domains,
        },
        tamper,
    })
}

fn checked_roots(copy_root: &Path, work_root: &Path) -> Result<(PathBuf, PathBuf), String> {
    let copy_root =
        fs::canonicalize(copy_root).map_err(|error| format!("resolve copied root: {error}"))?;
    let work_root = canonical_or_absolute(work_root)?;
    let copy = path_key(&copy_root);
    let work = path_key(&work_root);
    if contains_path(&copy, &work) || contains_path(&work, &copy) {
        return Err("copied root and work root must be disjoint".to_string());
    }
    Ok((copy_root, work_root))
}

fn canonical_or_absolute(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("read current directory: {error}"))?
            .join(path)
    };
    let absolute = normalize_path(&absolute);
    let mut ancestor = absolute.as_path();
    while fs::canonicalize(ancestor).is_err() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "work root has no existing ancestor".to_string())?;
    }
    let canonical_ancestor = fs::canonicalize(ancestor)
        .map_err(|error| format!("resolve work root ancestor: {error}"))?;
    let suffix = absolute
        .strip_prefix(ancestor)
        .map_err(|error| format!("resolve work root path: {error}"))?;
    Ok(canonical_ancestor.join(suffix))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn path_key(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('\\', "/");
    while value.len() > 1 && value.ends_with('/') {
        value.pop();
    }
    if cfg!(windows) {
        value.make_ascii_lowercase();
    }
    value
}

fn contains_path(parent: &str, child: &str) -> bool {
    parent == "/"
        || child == parent
        || child
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn hash_inventory(root: &Path, capture: &SourceCapture) -> Result<HashManifest, String> {
    let mut files = Vec::with_capacity(capture.files().len());
    for source in capture.files() {
        let Captured::Bytes { size, sha256 } = &source.captured else {
            return Err(format!(
                "cannot prove original unchanged: importer inventory entry {} is unreadable",
                source.relative
            ));
        };
        let path = inventoried_path(root, &source.relative)?;
        let (actual_size, actual_hash) = hash_file(root, &path)?;
        if actual_size != *size || actual_hash != sha256.to_hex() {
            return Err(format!(
                "original differs from its captured importer inventory entry: {}",
                source.relative
            ));
        }
        files.push(HashEntry {
            path: source.relative.clone(),
            size: *size,
            sha256: actual_hash,
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let encoded = serde_json::to_vec(&files).map_err(|error| error.to_string())?;
    Ok(HashManifest {
        sha256: format!("{:x}", Sha256::digest(encoded)),
        files,
    })
}

fn inventoried_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative_path = Path::new(relative);
    if relative_path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("invalid importer inventory path: {relative}"));
    }
    Ok(root.join(relative_path))
}

fn hash_file(root: &Path, path: &Path) -> Result<(u64, String), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("stat original inventory file: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("original importer inventory entry is not a regular file".to_string());
    }
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("resolve original inventory file: {error}"))?;
    if !canonical.starts_with(root) || canonical == root {
        return Err("original importer inventory entry escaped the copied root".to_string());
    }
    let mut file =
        File::open(path).map_err(|error| format!("open original inventory file: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("hash original inventory file: {error}"))?;
        if count == 0 {
            break;
        }
        size = size.saturating_add(count as u64);
        hasher.update(&buffer[..count]);
    }
    Ok((size, format!("{:x}", hasher.finalize())))
}

fn read_adapters(
    storage: &StorageOwner,
    logs_root: &Path,
    expected: &ImportPlan,
    comparison: &ComparisonReport,
) -> Result<Readback, String> {
    let desk = StorageDeskAdapter::from_storage(storage.clone());
    let actual_desk = desk.snapshot()?;
    let actual_identities = desk.identity_catalog()?;
    let (expected_desk, expected_identities) = expected_desk(expected)?;
    let desk_comparison = domain_comparison(comparison, ImportDomain::Desk)?;
    let mut desk_diffs = compare_desk(
        &expected_desk,
        &expected_identities,
        &actual_desk,
        &actual_identities,
    )?;
    let desk_entities = desk_entity_count(&actual_desk, actual_identities.len());
    let desk_items = desk_layout_count(&actual_desk);
    check_counts(
        ImportDomain::Desk,
        desk_comparison,
        desk_entities,
        desk_items,
        &mut desk_diffs,
    );

    let cron = CronStore::open_storage(
        storage,
        logs_root.to_path_buf(),
        system_clock(),
        CronStorageOptions { deadline: DEADLINE },
        crate::config::schema::AppConfig::default()
            .general_agent
            .agent_provider,
    )?;
    let expected_cron = expected_cron(expected)?;
    let cron_comparison = domain_comparison(comparison, ImportDomain::Cron)?;
    let (cron_histories, mut cron_diffs) = compare_cron(&expected_cron, &cron)?;
    let cron_entities = cron.list().len() as u64;
    let cron_items = cron_histories.values().map(|runs| runs.len() as u64).sum();
    check_counts(
        ImportDomain::Cron,
        cron_comparison,
        cron_entities,
        cron_items,
        &mut cron_diffs,
    );

    let sentinel = SentinelStore::with_storage(storage, system_clock());
    sentinel.ensure_available()?;
    let expected_sentinels = expected_sentinels(expected)?;
    let sentinel_comparison = domain_comparison(comparison, ImportDomain::Sentinel)?;
    let mut sentinel_diffs = compare_sentinels(&expected_sentinels, sentinel.list())?;
    let sentinel_entities = sentinel.list().len() as u64;
    let sentinel_items = sentinel
        .list()
        .iter()
        .map(|job| job.transitions.len() as u64)
        .sum();
    check_counts(
        ImportDomain::Sentinel,
        sentinel_comparison,
        sentinel_entities,
        sentinel_items,
        &mut sentinel_diffs,
    );

    let history_projection: BTreeMap<String, Vec<CronRunInfo>> = cron_histories.clone();
    let identity_projection = actual_identities
        .iter()
        .map(identity_value)
        .collect::<Vec<_>>();
    let projection = json!({
        "desk": { "snapshot": actual_desk, "identities": identity_projection },
        "cron": { "jobs": cron.list(), "history": history_projection },
        "sentinel": sentinel.list(),
    });
    let projection_sha256 = sha256_value(&projection)?;
    let domains = vec![
        domain_verification(desk_comparison, desk_entities, desk_items, desk_diffs),
        domain_verification(cron_comparison, cron_entities, cron_items, cron_diffs),
        domain_verification(
            sentinel_comparison,
            sentinel_entities,
            sentinel_items,
            sentinel_diffs,
        ),
    ];
    Ok(Readback {
        projection_sha256,
        domains,
    })
}

fn expected_desk(
    plan: &ImportPlan,
) -> Result<(SessionSnapshot, Vec<DeskAgentSessionIdentity>), String> {
    let mut snapshot = SessionSnapshot {
        version: "1".to_string(),
        timestamp: 0,
        workspaces: Vec::new(),
        active_workspace_id: None,
        teams: Vec::new(),
    };
    let selected = selected_relatives(plan, ImportDomain::Desk);
    if let Some(relative) = selected.iter().find(|relative| {
        **relative == "sessions/last.json" || relative.starts_with("sessions/backups/")
    }) {
        let bytes = captured_bytes(plan, relative)?;
        let text = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
        snapshot = legacy_decode::decode_snapshot(text)?.snapshot;
    }
    let identities = selected
        .iter()
        .find(|relative| relative.contains("agent-session-identities"))
        .map(|relative| {
            let bytes = captured_bytes(plan, relative)?;
            LegacyCodecs
                .decode_desk_identities(&bytes)
                .map_err(|error| error.detail)
        })
        .transpose()?
        .unwrap_or_default();
    Ok((snapshot, identities))
}

fn expected_cron(plan: &ImportPlan) -> Result<CronImport, String> {
    plan.domains()
        .iter()
        .find_map(|domain| match domain {
            DomainImport::Cron(import) => Some(import.clone()),
            _ => None,
        })
        .ok_or_else(|| "import plan has no cron domain".to_string())
}

fn expected_sentinels(plan: &ImportPlan) -> Result<Vec<Sentinel>, String> {
    let relative = selected_relatives(plan, ImportDomain::Sentinel)
        .into_iter()
        .next();
    relative
        .map(|relative| {
            let bytes = captured_bytes(plan, relative)?;
            serde_json::from_slice(&bytes)
                .map_err(|error| format!("decode expected sentinels: {error}"))
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn selected_relatives(plan: &ImportPlan, domain: ImportDomain) -> Vec<&str> {
    plan.provenance()
        .iter()
        .filter(|(found, _)| *found == domain)
        .filter_map(|(_, origin)| match origin {
            ImportProvenance::Current(relative) | ImportProvenance::Backup(relative) => {
                Some(relative.as_str())
            }
            ImportProvenance::FirstInstall | ImportProvenance::AcknowledgedAbsent => None,
        })
        .collect()
}

fn captured_bytes(plan: &ImportPlan, relative: &str) -> Result<Vec<u8>, String> {
    let source = plan
        .capture()
        .files()
        .iter()
        .find(|file| file.relative == relative)
        .ok_or_else(|| format!("selected importer file is absent: {relative}"))?;
    if !matches!(&source.captured, Captured::Bytes { .. }) {
        return Err(format!("selected importer file is unreadable: {relative}"));
    }
    fs::read(plan.capture().original_path(source))
        .map_err(|error| format!("read captured importer file {relative}: {error}"))
}

fn compare_desk(
    expected: &SessionSnapshot,
    expected_identities: &[DeskAgentSessionIdentity],
    actual: &SessionSnapshot,
    actual_identities: &[DeskAgentSessionIdentity],
) -> Result<DiffCollector, String> {
    let mut diffs = DiffCollector::new();
    for (subject, same) in [
        ("desk", expected.version == actual.version),
        ("desk", expected.timestamp == actual.timestamp),
        (
            "desk",
            expected.active_workspace_id == actual.active_workspace_id,
        ),
    ] {
        if !same {
            diffs.push(subject, "header");
        }
    }
    compare_order(
        "workspaces",
        expected.workspaces.iter().map(|item| item.id.as_str()),
        actual.workspaces.iter().map(|item| item.id.as_str()),
        &mut diffs,
    );
    let expected_workspaces: BTreeMap<&str, _> = expected
        .workspaces
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect();
    let actual_workspaces: BTreeMap<&str, _> = actual
        .workspaces
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect();
    for id in expected_workspaces
        .keys()
        .chain(actual_workspaces.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (expected_workspaces.get(id), actual_workspaces.get(id)) {
            (Some(want), Some(got)) => {
                let want_value = json!({
                    "name": want.name,
                    "active_tab_id": want.active_tab_id,
                    "cwd": want.cwd,
                    "provisioned_cwd": want.provisioned_cwd,
                    "protected": want.protected,
                    "sort_order": want.sort_order,
                    "task_keys": want.task_keys,
                });
                let got_value = json!({
                    "name": got.name,
                    "active_tab_id": got.active_tab_id,
                    "cwd": got.cwd,
                    "provisioned_cwd": got.provisioned_cwd,
                    "protected": got.protected,
                    "sort_order": got.sort_order,
                    "task_keys": got.task_keys,
                });
                if want_value != got_value {
                    diffs.push(format!("workspace:{id}"), "projection");
                }
                compare_tabs(id, &want.tabs, &got.tabs, &mut diffs)?;
            }
            (Some(_), None) => diffs.push(format!("workspace:{id}"), "missing"),
            (None, Some(_)) => diffs.push(format!("workspace:{id}"), "extra"),
            (None, None) => {}
        }
    }
    compare_order(
        "teams",
        expected.teams.iter().map(|item| item.id.as_str()),
        actual.teams.iter().map(|item| item.id.as_str()),
        &mut diffs,
    );
    compare_json_entities(
        expected
            .teams
            .iter()
            .map(|team| {
                Ok((
                    team.id.clone(),
                    serde_json::to_value(team).map_err(|e| e.to_string())?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?,
        actual
            .teams
            .iter()
            .map(|team| {
                Ok((
                    team.id.clone(),
                    serde_json::to_value(team).map_err(|e| e.to_string())?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?,
        "team",
        &mut diffs,
    );
    compare_json_entities(
        expected_identities
            .iter()
            .map(|identity| (identity_key(identity), identity_value(identity)))
            .collect(),
        actual_identities
            .iter()
            .map(|identity| (identity_key(identity), identity_value(identity)))
            .collect(),
        "identity",
        &mut diffs,
    );
    let expected_identity_order: Vec<_> = expected_identities.iter().map(identity_key).collect();
    let actual_identity_order: Vec<_> = actual_identities.iter().map(identity_key).collect();
    compare_order(
        "identities",
        expected_identity_order.iter().map(String::as_str),
        actual_identity_order.iter().map(String::as_str),
        &mut diffs,
    );
    Ok(diffs)
}

fn compare_tabs(
    workspace_id: &str,
    expected: &[codeterm_pty::session::store::TabSnapshot],
    actual: &[codeterm_pty::session::store::TabSnapshot],
    diffs: &mut DiffCollector,
) -> Result<(), String> {
    compare_order(
        &format!("tabs:{workspace_id}"),
        expected.iter().map(|item| item.id.as_str()),
        actual.iter().map(|item| item.id.as_str()),
        diffs,
    );
    let expected: BTreeMap<&str, _> = expected
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect();
    let actual: BTreeMap<&str, _> = actual.iter().map(|item| (item.id.as_str(), item)).collect();
    for id in expected
        .keys()
        .chain(actual.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (expected.get(id), actual.get(id)) {
            (Some(want), Some(got)) => {
                let want = serde_json::to_value(want).map_err(|error| error.to_string())?;
                let got = serde_json::to_value(got).map_err(|error| error.to_string())?;
                if want != got {
                    diffs.push(format!("tab:{workspace_id}/{id}"), "projection");
                }
            }
            (Some(_), None) => diffs.push(format!("tab:{workspace_id}/{id}"), "missing"),
            (None, Some(_)) => diffs.push(format!("tab:{workspace_id}/{id}"), "extra"),
            (None, None) => {}
        }
    }
    Ok(())
}

fn identity_key(identity: &DeskAgentSessionIdentity) -> String {
    format!(
        "{}:{}{}",
        identity.provider.len(),
        identity.provider,
        identity.session_id
    )
}

fn identity_value(identity: &DeskAgentSessionIdentity) -> Value {
    json!({
        "provider": identity.provider,
        "session_id": identity.session_id,
        "record_provider": identity.record_provider,
        "role": identity.role,
        "role_profile": identity.role_profile,
        "parent_tab_id": identity.parent_tab_id,
        "can_spawn": identity.can_spawn,
    })
}

fn expected_cron_jobs(import: &CronImport) -> Result<Vec<CronJob>, String> {
    import.jobs.iter().map(cron_job).collect()
}

fn cron_job(import: &CronJobImport) -> Result<CronJob, String> {
    let record = &import.record;
    Ok(CronJob {
        id: record.id.as_str().to_string(),
        name: record.name.clone(),
        description: record.description.clone(),
        workspace_id: record.workspace_id.clone(),
        team_id: record.team_id.clone(),
        schedule: record.schedule.clone(),
        enabled: record.enabled,
        agent: record.agent.clone(),
        model: record.model.clone(),
        reasoning_effort: record.reasoning_effort.clone(),
        task: record.task.clone(),
        cwd: record.cwd.clone(),
        role: record.role.clone(),
        skill: record.skill.clone(),
        creator_tab_id: record.creator_tab_id.clone(),
        report_target_tab_id: record.report_target_tab_id.clone(),
        oneshot: record.oneshot,
        max_history: usize::try_from(record.max_history)
            .map_err(|_| "cron max history exceeds this platform".to_string())?,
        created_at: record.created_at,
        last_run: import.last_run.as_ref().map(cron_run),
        next_run: record.next_run,
        jitter_max_ms: record.jitter_max_ms,
        tz: record.tz.clone(),
        timeout_seconds: record.timeout_seconds,
        consecutive_failures: record.consecutive_failures,
        protected: record.protected,
    })
}

fn cron_run(run: &CronRunRecord) -> CronRunInfo {
    CronRunInfo {
        run_id: run.run_id.as_str().to_string(),
        started_at: run.started_at,
        finished_at: run.finished_at,
        status: match run.status {
            CronRunState::Running => CronRunStatus::Running,
            CronRunState::AwaitingOwnerPrompt => CronRunStatus::AwaitingOwnerPrompt,
            CronRunState::Success => CronRunStatus::Success,
            CronRunState::Failed => CronRunStatus::Failed,
            CronRunState::Cancelled => CronRunStatus::Cancelled,
        },
        tab_id: run.tab_id.clone(),
        session_id: run.session_id.clone(),
        cwd: run.cwd.clone(),
        summary: run.summary.clone(),
        report_delivery: run.report_delivery.clone(),
    }
}

fn compare_cron(
    expected: &CronImport,
    actual: &CronStore,
) -> Result<(BTreeMap<String, Vec<CronRunInfo>>, DiffCollector), String> {
    let expected_jobs = expected_cron_jobs(expected)?;
    let mut diffs = DiffCollector::new();
    compare_entity_map(
        expected_jobs
            .iter()
            .map(|job| (job.id.clone(), job))
            .collect(),
        actual
            .list()
            .iter()
            .map(|job| (job.id.clone(), job))
            .collect(),
        "cron-job",
        &mut diffs,
    );
    compare_order(
        "cron-jobs",
        expected_jobs.iter().map(|job| job.id.as_str()),
        actual.list().iter().map(|job| job.id.as_str()),
        &mut diffs,
    );
    let expected_histories: BTreeMap<String, Vec<CronRunInfo>> = expected
        .histories
        .iter()
        .map(|history| {
            (
                history.job_id.as_str().to_string(),
                history.runs.iter().map(cron_run).collect(),
            )
        })
        .collect();
    let mut expected_histories = expected_histories;
    for job in &expected.jobs {
        expected_histories
            .entry(job.record.id.as_str().to_string())
            .or_default();
    }
    let mut history_ids: BTreeSet<String> = expected_histories.keys().cloned().collect();
    history_ids.extend(actual.list().iter().map(|job| job.id.clone()));
    let actual_histories: BTreeMap<String, Vec<CronRunInfo>> = history_ids
        .into_iter()
        .map(|id| {
            (
                id.clone(),
                actual.get_history(&id).into_iter().cloned().collect(),
            )
        })
        .collect();
    compare_entity_map(
        expected_histories.iter().collect(),
        actual_histories.iter().collect(),
        "cron-history",
        &mut diffs,
    );
    Ok((actual_histories, diffs))
}

fn compare_sentinels(expected: &[Sentinel], actual: &[Sentinel]) -> Result<DiffCollector, String> {
    let mut diffs = DiffCollector::new();
    compare_order(
        "sentinels",
        expected.iter().map(|job| job.id.as_str()),
        actual.iter().map(|job| job.id.as_str()),
        &mut diffs,
    );
    let expected_values = expected
        .iter()
        .map(|job| {
            Ok((
                job.id.clone(),
                serde_json::to_value(job).map_err(|e| e.to_string())?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let actual_values = actual
        .iter()
        .map(|job| {
            Ok((
                job.id.clone(),
                serde_json::to_value(job).map_err(|e| e.to_string())?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    compare_json_entities(expected_values, actual_values, "sentinel", &mut diffs);
    Ok(diffs)
}

fn compare_entity_map<'a, K, V>(
    expected: BTreeMap<K, &'a V>,
    actual: BTreeMap<K, &'a V>,
    prefix: &str,
    diffs: &mut DiffCollector,
) where
    K: Ord + Clone + ToString,
    V: PartialEq,
{
    for id in expected
        .keys()
        .chain(actual.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
    {
        match (expected.get(&id), actual.get(&id)) {
            (Some(want), Some(got)) if *want != *got => {
                diffs.push(format!("{prefix}:{}", id.to_string()), "projection")
            }
            (Some(_), None) => diffs.push(format!("{prefix}:{}", id.to_string()), "missing"),
            (None, Some(_)) => diffs.push(format!("{prefix}:{}", id.to_string()), "extra"),
            _ => {}
        }
    }
}

fn compare_json_entities(
    expected: Vec<(String, Value)>,
    actual: Vec<(String, Value)>,
    prefix: &str,
    diffs: &mut DiffCollector,
) {
    let expected: BTreeMap<String, Value> = expected.into_iter().collect();
    let actual: BTreeMap<String, Value> = actual.into_iter().collect();
    for id in expected
        .keys()
        .chain(actual.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
    {
        match (expected.get(&id), actual.get(&id)) {
            (Some(want), Some(got)) if want != got => {
                diffs.push(format!("{prefix}:{id}"), "projection")
            }
            (Some(_), None) => diffs.push(format!("{prefix}:{id}"), "missing"),
            (None, Some(_)) => diffs.push(format!("{prefix}:{id}"), "extra"),
            _ => {}
        }
    }
}

fn compare_order<'a>(
    subject: &str,
    expected: impl Iterator<Item = &'a str>,
    actual: impl Iterator<Item = &'a str>,
    diffs: &mut DiffCollector,
) {
    if expected.collect::<Vec<_>>() != actual.collect::<Vec<_>>() {
        diffs.push(subject, "order");
    }
}

fn domain_comparison(
    comparison: &ComparisonReport,
    domain: ImportDomain,
) -> Result<&DomainComparison, String> {
    comparison
        .domains
        .iter()
        .find(|item| item.domain == domain)
        .ok_or_else(|| format!("importer comparison has no {} domain", domain.as_str()))
}

fn check_counts(
    domain: ImportDomain,
    comparison: &DomainComparison,
    adapter_entities: u64,
    adapter_items: u64,
    diffs: &mut DiffCollector,
) {
    if comparison.expected_entities != adapter_entities {
        diffs.push(domain.as_str(), "adapter_entity_count");
    }
    if comparison.expected_items != adapter_items {
        diffs.push(domain.as_str(), "adapter_item_count");
    }
}

fn domain_verification(
    comparison: &DomainComparison,
    adapter_entities: u64,
    adapter_items: u64,
    adapter_diffs: DiffCollector,
) -> DomainVerification {
    DomainVerification {
        domain: comparison.domain.as_str().to_string(),
        expected_entities: comparison.expected_entities,
        committed_entities: comparison.committed_entities,
        adapter_entities,
        expected_items: comparison.expected_items,
        committed_items: comparison.committed_items,
        adapter_items,
        importer_diffs: comparison
            .mismatches
            .iter()
            .map(|mismatch| EntityDiff {
                subject: mismatch.subject.clone(),
                field: mismatch.field.to_string(),
            })
            .collect(),
        importer_diffs_elided: comparison.elided,
        adapter_diffs: adapter_diffs.diffs,
        adapter_diffs_elided: adapter_diffs.elided,
    }
}

fn readback_clean(readback: &Readback) -> bool {
    readback.domains.iter().all(|domain| {
        domain.expected_entities == domain.committed_entities
            && domain.expected_entities == domain.adapter_entities
            && domain.expected_items == domain.committed_items
            && domain.expected_items == domain.adapter_items
            && domain.importer_diffs.is_empty()
            && domain.importer_diffs_elided == 0
            && domain.adapter_diffs.is_empty()
            && domain.adapter_diffs_elided == 0
    })
}

fn desk_entity_count(snapshot: &SessionSnapshot, identities: usize) -> u64 {
    snapshot.workspaces.len() as u64
        + snapshot
            .workspaces
            .iter()
            .map(|workspace| workspace.tabs.len() as u64)
            .sum::<u64>()
        + snapshot.teams.len() as u64
        + identities as u64
}

fn desk_layout_count(snapshot: &SessionSnapshot) -> u64 {
    snapshot
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.tabs.iter())
        .map(|tab| layout_node_count(&tab.split_tree))
        .sum()
}

fn layout_node_count(node: &codeterm_pty::session::store::SplitNodeSnapshot) -> u64 {
    match node {
        codeterm_pty::session::store::SplitNodeSnapshot::Leaf { .. } => 1,
        codeterm_pty::session::store::SplitNodeSnapshot::Split { children, .. } => {
            1 + children.iter().map(layout_node_count).sum::<u64>()
        }
    }
}

fn sha256_value(value: &Value) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn run_tamper_leg(
    copy_root: &Path,
    work_root: &Path,
    original_hashes: &HashManifest,
) -> Result<TamperVerification, String> {
    let archive = work_root.join("tamper-archive");
    let capture = capture_sources(copy_root, &archive, Default::default())
        .map_err(|error| format!("capture tamper leg: {error}"))?;
    let before = hash_inventory(copy_root, &capture)?;
    let plan = ImportPlan::resolve(capture, &LegacyCodecs, &BTreeMap::new())
        .map_err(|error| format!("resolve tamper leg: {error}"))?;
    if before != *original_hashes {
        return Err("original inventory changed before the tamper leg".to_string());
    }
    tamper_capture(plan.capture())?;
    let tamper_storage = Storage::open(work_root.join("tamper-instance"), StorageConfig::default())
        .map_err(|error| format!("open tamper storage: {error}"))?;
    let result = tamper_storage.commit_legacy_import(plan, DEADLINE);
    let archive_tamper_rejected = matches!(
        result.as_ref().err().and_then(|error| error.refusal()),
        Some(ImportRefusal::BackupTampered { .. })
    );
    let marker_absent = tamper_storage
        .legacy_import_marker()
        .wait(DEADLINE)
        .map_err(|error| format!("read tamper marker: {error}"))?
        .value
        .is_none();
    tamper_storage
        .shutdown()
        .wait(DEADLINE)
        .map_err(|error| format!("shutdown tamper storage: {error}"))?;
    drop(tamper_storage);
    let after_capture = capture_sources(
        copy_root,
        &work_root.join("tamper-after-capture"),
        Default::default(),
    )
    .map_err(|error| format!("capture originals after tamper leg: {error}"))?;
    let after = hash_inventory(copy_root, &after_capture)?;
    Ok(TamperVerification {
        archive_tamper_rejected,
        marker_absent,
        originals_unchanged: before == after,
    })
}

#[cfg(unix)]
fn enable_writes(permissions: &mut fs::Permissions) {
    use std::os::unix::fs::PermissionsExt;

    permissions.set_mode(permissions.mode() | 0o200);
}

#[cfg(windows)]
#[allow(clippy::permissions_set_readonly_false)]
fn enable_writes(permissions: &mut fs::Permissions) {
    // The verifier mutates only its copied tamper fixture.
    permissions.set_readonly(false);
}

fn tamper_capture(capture: &SourceCapture) -> Result<(), String> {
    let source = capture
        .files()
        .iter()
        .find(|source| matches!(&source.captured, Captured::Bytes { .. }))
        .ok_or_else(|| "tamper leg has no captured source file".to_string())?;
    let path = capture.original_path(source);
    let mut permissions = fs::metadata(&path)
        .map_err(|error| format!("read captured file permissions: {error}"))?
        .permissions();
    enable_writes(&mut permissions);
    fs::set_permissions(&path, permissions)
        .map_err(|error| format!("make captured tamper file writable: {error}"))?;
    let mut bytes =
        fs::read(&path).map_err(|error| format!("read captured tamper file: {error}"))?;
    if bytes.is_empty() {
        bytes.push(1);
    } else {
        bytes[0] ^= 1;
    }
    let write_result = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&path)
        .and_then(|mut file| file.write_all(&bytes));
    let mut permissions = fs::metadata(&path)
        .map_err(|error| format!("read tamper file permissions: {error}"))?
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions)
        .map_err(|error| format!("seal tampered archive file: {error}"))?;
    write_result.map_err(|error| format!("tamper captured archive: {error}"))
}

#[cfg(test)]
mod tests;
