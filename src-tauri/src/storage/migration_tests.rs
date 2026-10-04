//! storage_migration_: the legacy importer over synthetic inert copies of an instance root,
//! driving the real cron and Sentinel codecs through the shared Storage owner. Fixtures
//! are opaque and secret-free; every path lives under a fresh temporary directory.

use super::migration::{import_copied_root, LegacyCodecs};
use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

mod bounds;
mod capture;
mod conservation;
mod desk;
mod live_root;
mod selection;
mod transaction;

/// Failure bound around event waits only; elapsed time never signals readiness.
const DEADLINE: Duration = Duration::from_secs(30);

/// Two divergent last-run pairs, a last run outside its history, an explicitly empty live
/// history and history that outlived its deleted job.
const CRONS: &str = r#"{
  "version": "1",
  "jobs": [
    {"id": "stale-mirror", "name": "stale mirror", "workspace_id": "ws-a", "team_id": "team-a",
     "schedule": "0 * * * *", "enabled": true, "agent": "claude", "model": "opus",
     "task": "check the build", "created_at": 1, "max_history": 5, "jitter_max_ms": 0,
     "next_run": 5, "consecutive_failures": 2, "tz": "Europe/Riga", "timeout_seconds": 600,
     "last_run": {"run_id": "r1", "started_at": 10, "finished_at": null,
       "status": "Running", "tab_id": "tab-r1", "session_id": null, "summary": null}},
    {"id": "summary-drift", "name": "summary drift", "workspace_id": "ws-b",
     "schedule": "*/5 * * * *", "enabled": false, "agent": "codex", "task": "t",
     "description": "drifted summary", "created_at": 2, "jitter_max_ms": 0,
     "last_run": {"run_id": "s1", "started_at": 20, "finished_at": 21,
       "status": "Success", "tab_id": null}},
    {"id": "unlisted", "name": "unlisted last run", "workspace_id": "ws-a",
     "schedule": "in 10m", "enabled": true, "agent": "claude", "task": "once",
     "oneshot": true, "protected": true, "created_at": 3, "next_run": 7,
     "last_run": {"run_id": "u1", "started_at": 30, "finished_at": 31, "status": "Failed",
       "tab_id": "tab-u1", "summary": "boom", "report_delivery": "queued"}},
    {"id": "quiet", "name": "quiet", "workspace_id": "ws-c", "schedule": "0 0 * * *",
     "enabled": true, "agent": "claude", "task": "nothing yet", "created_at": 4}
  ],
  "history": {
    "stale-mirror": [
      {"run_id": "r0", "started_at": 5, "finished_at": 6, "status": "Success", "tab_id": null},
      {"run_id": "r1", "started_at": 10, "finished_at": 15, "status": "Cancelled",
       "tab_id": null, "summary": "cancelled by owner"}
    ],
    "summary-drift": [
      {"run_id": "s1", "started_at": 20, "finished_at": 21, "status": "Success",
       "tab_id": null, "summary": "done"}
    ],
    "quiet": [],
    "deleted-job": [
      {"run_id": "d1", "started_at": 1, "finished_at": 2, "status": "Success",
       "tab_id": null, "session_id": "sess-d1", "cwd": "/projects/old"}
    ]
  }
}"#;

/// Scenario root and child, a stopped solo stage, unconfirmed claims, a command result and
/// a cron handler whose job the cron domain does not hold.
const SENTINELS: &str = r#"[
 {"id":"sentinel_root","team_id":"team","creator_tab_id":"creator","workspace_id":"workspace",
  "cwd":"/projects/one","probe":"status --short","question":"Which state?",
  "buckets":["done","failed","waiting"],"interval_ms":20000,
  "routes":{
   "done":[
    {"id":"join","action":{"kind":"sentinel","target":"sentinel_child"},
     "target":{"kind":"sentinel","id":"sentinel_child","workspace_id":"workspace","team_id":"team","owner_tab_id":"creator"}},
    {"id":"build","action":{"kind":"command","target":{"command":"make check","label":"Check","timeout_ms":1000}},"target":null},
    {"id":"tell","action":{"kind":"send","target":"worker-tab"},
     "target":{"kind":"worker","id":"worker-tab","workspace_id":"workspace","team_id":"team","owner_tab_id":null}}],
   "failed":[
    {"id":"nightly","action":{"kind":"cron","target":"nightly"},
     "target":{"kind":"cron","id":"nightly","workspace_id":"workspace","team_id":"team","owner_tab_id":"creator"}},
    {"id":"halt","action":{"kind":"stop"},"target":null}],
   "waiting":[]},
  "matches":{"done":"^done$"},"until":"failed","expires_at":9000000,"next_run":21000,
  "machine":{"last_hash":"abc123","last_bucket":"done","stopped":false,"paused":false},
  "last_reason":"transition","revision":7,"agent_authored":true,"name":"Supplied scenario",
  "scenario_id":"sentinel_root","max_transitions":8,"max_actions":16,
  "transitions":[
   {"id":"eval-1","bucket":"done","accepted_at":1500,"terminal":false,"handlers":[
    {"handler_id":"join","status":"noop","claim_id":"claim-1","execution_id":"claim-1","accepted_at":1501,"reason":null},
    {"handler_id":"build","status":"failed","claim_id":"claim-2","execution_id":"exec-2","accepted_at":1502,
     "command_result":{"status":"nonzero","exit_code":2,"resources_confirmed":true,
      "stdout":{"total_bytes":10,"retained_bytes":10,"truncated":false},
      "stderr":{"total_bytes":40000,"retained_bytes":32768,"truncated":true},
      "started_at":1502,"finished_at":1503,"primary":null,"cleanup":[]},
     "reason":"action_failed"},
    {"handler_id":"tell","status":"skipped","claim_id":null,"execution_id":null,"accepted_at":null,"reason":"batch_fenced"}]},
   {"id":"eval-2","bucket":"done","accepted_at":1600,"terminal":false,"handlers":[
    {"handler_id":"join","status":"unconfirmed","claim_id":"claim-3","execution_id":null,"accepted_at":1601,"reason":null}]}]},
 {"id":"sentinel_child","team_id":"team","creator_tab_id":"creator","workspace_id":"workspace",
  "cwd":"/projects/one","probe":"status","question":"Child state?","buckets":["done"],
  "interval_ms":10000,"routes":{"done":[]},"matches":{},"until":null,"expires_at":null,
  "next_run":1000,"machine":{"last_hash":null,"last_bucket":null,"stopped":false,"paused":true},
  "last_reason":null,"revision":0,"agent_authored":true,"name":null,
  "scenario_id":"sentinel_root","max_transitions":64,"max_actions":256,"transitions":[]},
 {"id":"sentinel_solo","team_id":null,"creator_tab_id":"solo-creator","workspace_id":"workspace",
  "cwd":"/projects/two","probe":"tail log","question":"Late?","buckets":["late"],
  "interval_ms":10000,
  "routes":{"late":[{"id":"probe-cmd","action":{"kind":"command","target":{"command":"printf late","label":"Late","timeout_ms":1000}},"target":null}]},
  "matches":{"late":"late"},"until":null,"expires_at":1200,"next_run":1000,
  "machine":{"last_hash":"def456","last_bucket":"late","stopped":true,"paused":false},
  "last_reason":"timeout","revision":3,"agent_authored":false,"name":null,"scenario_id":null,
  "max_transitions":64,"max_actions":256,
  "transitions":[{"id":"eval-9","bucket":"late","accepted_at":1100,"terminal":true,"handlers":[
   {"handler_id":"probe-cmd","status":"unconfirmed","claim_id":"claim-9","execution_id":"exec-9","accepted_at":1101,
    "command_result":{"status":"unconfirmed","exit_code":null,"resources_confirmed":false,"stdout":null,"stderr":null,
     "started_at":1101,"finished_at":1102,"primary":"probe_completion_unconfirmed",
     "cleanup":["child process still running","pipe still open"]},
    "reason":"effect_unconfirmed"}]}]}
]"#;

const EMPTY_CRONS: &str = r#"{"version": "1", "jobs": [], "history": {}}"#;

const DESK_SOURCE: &str = r#"{
  "version":"legacy",
  "timestamp":91,
  "active_workspace_id":"ws-a",
  "teams":[
    {"id":"team-a","name":"Alpha","lifecycle":"active","created_at":11,"workspace_id":"ws-a"},
    {"id":"team-historical","name":null,"lifecycle":"closing","created_at":12,"workspace_id":"deleted-workspace"}
  ],
  "workspaces":[
    {"id":"ws-a","name":"Alpha","tabs":[
      {"id":"group-a","title":"Group","description":null,"sort_order":3,
       "split_tree":{"type":"Split","orientation":"horizontal","ratio":0.4,
        "future_split_state":{"flag":true,"nullable":null},"children":[
         {"type":"Leaf","tab_id":"tab-a","cwd":"C:/projects/a","title":null,
          "agent_type":"provider-unverified","claude_session_id":"session-unverified",
          "agent_identity":{"session_binding_nonce":"nonce-unverified"},"role":"Worker",
          "role_profile":null,"agent_session_model":null,"can_spawn":true},
         {"type":"Split","orientation":"vertical","ratio":0.7,
          "future_split_ratio":null,"children":[
           {"type":"Leaf","tab_id":"missing-tab","agent_type":"unknown-provider","role":"Manager"}
         ]}
       ]}},
      {"id":"tab-a","title":"Second","description":"kept","sort_order":7,
       "split_tree":{"type":"Leaf","tab_id":"tab-a","cwd":null,"title":"Second"}}
    ],"active_tab_id":"group-a","cwd":"C:/projects/a","provisioned_cwd":null,
       "protected":true,"sort_order":8,"task_keys":{"prefix":"COD","next":4}},
    {"id":"ws-b","name":"Beta","tabs":[
      {"id":"tab-b","title":"Beta tab","sort_order":1,
       "split_tree":{"type":"Leaf","tab_id":"tab-b","cwd":null}}
    ],"active_tab_id":null,"cwd":null,"provisioned_cwd":"C:/projects/b",
       "protected":false,"sort_order":2,"task_keys":{"prefix":null,"next":0}}
  ]
}"#;

const IDENTITY_SOURCE: &str = r#"{"sessions":{"6:claudehistory-session":{"provider":"codex","role":"Worker","role_profile":null,"parent_tab_id":"deleted-tab","can_spawn":false}}}"#;
const IDENTITY_BACKUP: &str = r#"{"sessions":{"6:claudeold-session":{"provider":"claude","role":"Planner","role_profile":"planner","parent_tab_id":null,"can_spawn":true}}}"#;
const NAMED_DESK: &str =
    r#"{"version":"saved","timestamp":9,"active_workspace_id":null,"teams":[],"workspaces":[]}"#;

/// A temporary tree holding an inert source copy, its backup and a separate instance root.
struct Fixture {
    temp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temp root");
        fs::create_dir_all(temp.path().join("copy")).expect("source copy");
        fs::create_dir_all(temp.path().join("instance")).expect("instance root");
        Fixture { temp }
    }

    fn seeded() -> Self {
        let fixture = Fixture::new();
        fixture.put("crons.json", CRONS.as_bytes());
        fixture.put("sentinels.json", SENTINELS.as_bytes());
        fixture
    }

    fn seeded_with_desk() -> Self {
        let fixture = Self::seeded();
        fixture.put("sessions/last.json", DESK_SOURCE.as_bytes());
        fixture.put(
            "sessions/agent-session-identities.json",
            IDENTITY_SOURCE.as_bytes(),
        );
        fixture.put(
            "sessions/agent-session-identities.json.bak",
            IDENTITY_BACKUP.as_bytes(),
        );
        fixture.put("sessions/named.json", NAMED_DESK.as_bytes());
        fixture.put("sessions/backups/1730000000.json", NAMED_DESK.as_bytes());
        fixture.put("sessions/last.json.tmp", b"interrupted");
        fixture.put("sessions/last.json.corrupt-1730000000", b"corrupt evidence");
        fixture.put("sessions/last.json.reduced-1730000000", b"reduced evidence");
        fixture.put(
            "sessions/agent-session-identities.json.tmp",
            b"interrupted identities",
        );
        fixture
    }

    fn source(&self) -> PathBuf {
        self.temp.path().join("copy")
    }

    fn backup(&self) -> PathBuf {
        self.temp.path().join("backup")
    }

    fn instance(&self) -> PathBuf {
        self.temp.path().join("instance")
    }

    fn other(&self, name: &str) -> PathBuf {
        self.temp.path().join(name)
    }

    /// Writes into the synthetic source copy only.
    fn put(&self, relative: &str, bytes: &[u8]) {
        let path = self.source().join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("source dirs");
        fs::write(path, bytes).expect("source file");
    }

    fn open(&self) -> Storage {
        Storage::open(self.instance(), StorageConfig::default()).expect("open storage")
    }

    fn request(&self) -> ImportRequest {
        ImportRequest::new(self.source(), self.backup())
    }

    fn capture(&self) -> SourceCapture {
        capture_sources(&self.source(), &self.backup(), CaptureLimits::default()).expect("capture")
    }

    fn plan(&self) -> ImportPlan {
        ImportPlan::resolve(self.capture(), &LegacyCodecs, &BTreeMap::new()).expect("plan")
    }

    fn select(&self, domain: ImportDomain, selection: SourceSelection) -> ImportRequest {
        let mut request = self.request();
        request.selections.insert(domain, selection);
        request
    }
}

fn close(storage: Storage) {
    storage.shutdown().wait(DEADLINE).expect("storage shutdown");
}

fn committed(outcome: Result<ImportOutcome, MigrationError>) -> ImportReceipt {
    match outcome.expect("import") {
        ImportOutcome::Committed(receipt) => *receipt,
        other => panic!("expected a fresh commit, got {other:?}"),
    }
}

fn refusal(error: MigrationError) -> ImportRefusal {
    error
        .refusal()
        .cloned()
        .unwrap_or_else(|| panic!("expected a refusal, got {error:?}"))
}

fn marker(storage: &Storage) -> Option<ImportMarker> {
    storage
        .legacy_import_marker()
        .wait(DEADLINE)
        .expect("marker read")
        .value
}

fn cron_snapshot(storage: &Storage) -> CronSnapshot {
    storage
        .crons()
        .snapshot()
        .wait(DEADLINE)
        .expect("cron snapshot")
        .value
}

fn sentinel_inventory(storage: &Storage) -> Vec<StoredSentinel> {
    storage
        .sentinels()
        .inventory()
        .wait(DEADLINE)
        .expect("sentinel inventory")
        .value
}

/// Nothing committed: no marker and empty domain repositories.
fn assert_untouched(storage: &Storage) {
    assert_eq!(marker(storage), None, "no completion marker");
    assert!(cron_snapshot(storage).states.is_empty(), "no cron rows");
    assert!(sentinel_inventory(storage).is_empty(), "no Sentinel rows");
    assert!(
        storage
            .workspaces()
            .list()
            .wait(DEADLINE)
            .expect("workspace inventory")
            .value
            .workspaces
            .is_empty(),
        "no workspace rows"
    );
    assert!(
        matches!(
            storage.desk().snapshot().wait(DEADLINE),
            Err(StorageError::NotFound { .. })
        ),
        "no desk metadata"
    );
}

fn decoded_cron() -> CronImport {
    crate::cron::store::decode_legacy_cron_file(CRONS.as_bytes()).expect("cron fixture")
}

fn decoded_sentinels() -> Vec<SentinelImport> {
    crate::sentinel::storage::legacy_import(SENTINELS).expect("sentinel fixture")
}

/// Every file under `root` by forward-slash relative path.
fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    walk(root, root, &mut |relative: String, path: &Path| {
        files.insert(relative, fs::read(path).expect("read file"));
    });
    files
}

fn stamps(root: &Path) -> BTreeMap<String, SystemTime> {
    let mut found = BTreeMap::new();
    walk(root, root, &mut |relative: String, path: &Path| {
        let modified = fs::metadata(path)
            .and_then(|meta| meta.modified())
            .expect("modified time");
        found.insert(relative, modified);
    });
    found
}

fn walk(root: &Path, dir: &Path, visit: &mut dyn FnMut(String, &Path)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            walk(root, &path, visit);
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .expect("under root")
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        visit(relative, &path);
    }
}

fn reference_sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Lets a test alter its own sealed backup copy.
#[allow(clippy::permissions_set_readonly_false)]
fn make_writable(path: &Path) {
    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    #[cfg(unix)]
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o600);
    #[cfg(not(unix))]
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).expect("unseal");
}

/// Manifest lines split on the single spaces the format guarantees.
fn lines(bytes: &[u8]) -> Vec<Vec<String>> {
    String::from_utf8(bytes.to_vec())
        .expect("utf-8 manifest")
        .lines()
        .map(|line| line.split(' ').map(str::to_string).collect())
        .collect()
}

fn line(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

/// Hands back fixed typed imports, so preflight bounds can be driven past what a source
/// decoder would ever produce.
struct Fixed {
    cron: CronImport,
    sentinel: Vec<SentinelImport>,
}

impl Fixed {
    fn cron(cron: CronImport) -> Self {
        Fixed {
            cron,
            sentinel: Vec::new(),
        }
    }

    fn sentinel(sentinel: Vec<SentinelImport>) -> Self {
        Fixed {
            cron: CronImport::default(),
            sentinel,
        }
    }
}

impl LegacyDecoder for Fixed {
    fn decode(&self, domain: ImportDomain, _bytes: &[u8]) -> Result<DomainImport, DecodeFailure> {
        match domain {
            ImportDomain::Cron => Ok(DomainImport::Cron(self.cron.clone())),
            ImportDomain::Sentinel => Ok(DomainImport::Sentinel(self.sentinel.clone())),
            _ => Err(DecodeFailure::new(
                DecodeFailureKind::Unsupported,
                "fixture",
            )),
        }
    }
}

fn job(id: &str) -> CronJobImport {
    CronJobImport {
        record: CronJobRecord {
            id: CronJobId::new(id).expect("job id"),
            name: format!("job {id}"),
            description: None,
            workspace_id: "ws".to_string(),
            team_id: None,
            schedule: "0 * * * *".to_string(),
            enabled: true,
            agent: "claude".to_string(),
            model: None,
            reasoning_effort: None,
            task: "t".to_string(),
            cwd: None,
            role: None,
            skill: None,
            creator_tab_id: None,
            report_target_tab_id: None,
            oneshot: false,
            max_history: 100,
            created_at: 1,
            last_run_id: None,
            next_run: Some(1),
            jitter_max_ms: None,
            tz: None,
            timeout_seconds: None,
            consecutive_failures: 0,
            protected: false,
        },
        last_run: None,
    }
}

fn run(id: &str, started_at: u64) -> CronRunRecord {
    CronRunRecord {
        run_id: CronRunId::new(id).expect("run id"),
        started_at,
        finished_at: Some(started_at + 1),
        status: CronRunState::Success,
        tab_id: None,
        session_id: None,
        cwd: None,
        summary: None,
        report_delivery: None,
    }
}

/// `count` distinct stages derived from the stopped solo fixture stage.
fn stages(count: usize) -> Vec<SentinelImport> {
    let solo = decoded_sentinels()
        .into_iter()
        .find(|record| record.identity.id.as_str() == "sentinel_solo")
        .expect("solo stage");
    (0..count)
        .map(|index| {
            let mut stage = solo.clone();
            stage.identity.id = SentinelId::new(format!("stage-{index}")).expect("stage id");
            stage
        })
        .collect()
}

/// Imports the seeded fixture through the production entry point.
fn import_seeded(fixture: &Fixture, storage: &Storage) -> ImportReceipt {
    committed(import_copied_root(storage, &fixture.request()))
}
