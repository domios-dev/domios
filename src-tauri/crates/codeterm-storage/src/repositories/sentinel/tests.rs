//! Source contract: input definitions round-trip, the columns refuse zero or two sources,
//! and the pre-source-pair rebuild keeps existing probe/observe rows byte-for-byte.

use super::*;
use crate::assets::{self, Layout};
use crate::schema::open_existing;
use crate::{Storage, StorageConfig};
use codeterm_types::blueprint_pipeline::{
    BlueprintOwner, BlueprintResult, BlueprintSpec, IoType, ResultTarget, Step, StepKind, Trigger,
};
use codeterm_types::sentinel_input::{DataRef, InputSpec, MediaKind};
use rusqlite::{params, Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "codeterm-storage-sentinel-{}-{stamp}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempRoot(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn storage() -> (TempRoot, Storage) {
    let root = TempRoot::new();
    let storage = Storage::open(root.path(), StorageConfig::default()).unwrap();
    (root, storage)
}

fn raw(root: &Path) -> Connection {
    let layout = Layout::of_root(&assets::canonical_dir(root).unwrap());
    open_existing(&layout.database, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap()
}

fn identity(id: &str) -> SentinelIdentity {
    SentinelIdentity {
        id: SentinelId::new(id).unwrap(),
        scope: SentinelScope {
            creator_tab_id: "tab-author".into(),
            workspace_id: "ws-alpha".into(),
            team_id: None,
        },
        cwd: std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        agent_authored: false,
    }
}

fn runtime() -> StageRuntime {
    StageRuntime {
        runs_since_transition: 0,
        last_probe_at: None,
        probe_state: None,
        causal_run_json: None,
        generation: 0,
        next_run: 1_000,
        expires_at: None,
        paused: false,
        stopped: false,
        last_hash: None,
        last_bucket: None,
        last_reason: None,
        held: None,
        run: None,
        run_transitions: 0,
    }
}

fn definition(source: StageSource) -> StageDefinition {
    StageDefinition {
        model_classification: false,
        session: false,
        name: Some("source-round-trip".into()),
        source,
        question: "did it finish?".into(),
        interval_ms: 20_000,
        until: Some("done".into()),
        escalation_timeout_ms: None,
        max_transitions: 8,
        max_actions: 16,
        buckets: vec![BucketDefinition {
            name: "done".into(),
            match_pattern: Some("^done$".into()),
            rearm_on_change: false,
            handlers: Vec::new(),
        }],
        on_other: Vec::new(),
    }
}

fn id(name: &str) -> SentinelId {
    SentinelId::new(name).unwrap()
}

#[test]
fn sentinel_input_definition_round_trips_insert_select_and_update() {
    let (_root, storage) = storage();
    let repo = storage.sentinels();
    let input = InputSpec {
        kind: MediaKind::Audio,
        data: DataRef::File(std::env::current_dir().unwrap().join("voice.wav")),
        convert_to: Some(MediaKind::Text),
        convert_timeout_ms: Some(30_000),
    };
    repo.create(SentinelCreate {
        identity: identity("sentinel_input_round_trip"),
        scenario_id: None,
        definition: definition(StageSource::Input(input.clone())),
        runtime: runtime(),
        associate: Vec::new(),
    })
    .wait(Duration::from_secs(5))
    .unwrap();

    let stored = repo
        .get(id("sentinel_input_round_trip"))
        .wait(Duration::from_secs(5))
        .unwrap()
        .value
        .expect("stored stage");
    assert_eq!(
        stored.record.definition.definition.source,
        StageSource::Input(input.clone())
    );
    assert_eq!(stored.record.definition.version, 1);

    let converted = InputSpec {
        kind: MediaKind::Audio,
        data: DataRef::Url("https://example.com/voice.wav".into()),
        convert_to: Some(MediaKind::Text),
        convert_timeout_ms: None,
    };
    let next = definition(StageSource::Input(converted.clone()));
    repo.update_definition(
        id("sentinel_input_round_trip"),
        1,
        DefinitionUpdate {
            definition: next.clone(),
            author_tab_id: "tab-author".into(),
            cause_transition_id: None,
            authored_at: 5_000,
        },
    )
    .wait(Duration::from_secs(5))
    .unwrap();

    let head = repo
        .get(id("sentinel_input_round_trip"))
        .wait(Duration::from_secs(5))
        .unwrap()
        .value
        .expect("stored stage");
    assert_eq!(head.record.definition.version, 2);
    assert_eq!(head.record.definition.definition.source, next.source);
    assert_eq!(
        head.record.definition.amendment.as_ref().unwrap().version,
        2
    );

    let history = repo
        .history(id("sentinel_input_round_trip"))
        .wait(Duration::from_secs(5))
        .unwrap()
        .value;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].definition.source, StageSource::Input(input));
    assert_eq!(history[1].definition.source, StageSource::Input(converted));
}

#[test]
fn blueprint_pipeline_definition_round_trips_through_versioned_storage() {
    let (_root, storage) = storage();
    let repo = storage.sentinels();
    let pipeline = BlueprintSpec {
        name: "echo pipeline".into(),
        owner: BlueprintOwner::Team,
        trigger: Trigger::Schedule {
            every: "20s".into(),
        },
        steps: vec![Step {
            name: "emit".into(),
            input: IoType::Text,
            kind: StepKind::Command {
                command: "printf ready".into(),
            },
            on: Default::default(),
        }],
        result: BlueprintResult {
            to: ResultTarget::Notify,
        },
    };
    let mut definition = definition(StageSource::Probe("blueprint-pipeline".into()));
    definition.source = StageSource::Pipeline(pipeline.clone());
    repo.create(SentinelCreate {
        identity: identity("blueprint_pipeline_round_trip"),
        scenario_id: None,
        definition,
        runtime: runtime(),
        associate: Vec::new(),
    })
    .wait(Duration::from_secs(5))
    .unwrap();

    let stored = repo
        .history(id("blueprint_pipeline_round_trip"))
        .wait(Duration::from_secs(5))
        .unwrap()
        .value;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].definition.pipeline().cloned(), Some(pipeline));
}

#[test]
fn sentinel_source_columns_refuse_zero_or_two_sources_and_admit_event() {
    let root = TempRoot::new();
    drop(Storage::open(root.path(), StorageConfig::default()).unwrap());
    let conn = raw(root.path());
    conn.execute(
        "INSERT INTO blueprints (id, ordinal, creator_tab_id, workspace_id, team_id, cwd, \
         agent_authored, scenario_id, revision) \
         VALUES ('sentinel_cols', 0, 'tab-author', 'ws-alpha', NULL, 'C:/work', 0, NULL, 1)",
        [],
    )
    .unwrap();
    let insert = |version: i64,
                  kind: &str,
                  spec: Option<&str>,
                  probe: &str,
                  observe: Option<(&str, &str)>|
     -> rusqlite::Result<usize> {
        let (observe_kind, observe_ref) = match observe {
            Some((kind, reference)) => (Some(kind), Some(reference)),
            None => (None, None),
        };
        conn.execute(
            "INSERT INTO blueprint_definitions (blueprint_id, version, provenance, name, \
             source_kind, source_spec, probe, observe_kind, observe_ref, question, interval_ms, \
             until_bucket, escalation_timeout_ms, max_transitions, max_actions, replaced_version, \
             author_tab_id, cause_transition_id, authored_at, created_revision) \
             VALUES ('sentinel_cols', ?1, 'authored', 'cols', ?2, ?3, ?4, ?5, ?6, 'q', 20000, \
             NULL, NULL, 8, 16, NULL, NULL, NULL, NULL, 1)",
            params![version, kind, spec, probe, observe_kind, observe_ref],
        )
    };
    let mut version = 0;
    let mut refused =
        |kind: &str, spec: Option<&str>, probe: &str, observe: Option<(&str, &str)>| {
            version += 1;
            let outcome = insert(version, kind, spec, probe, observe);
            assert!(
                outcome.is_err(),
                "expected refusal for {kind} {spec:?} {probe:?} {observe:?}"
            );
        };

    refused("probe", None, "", None);
    refused("observe", None, "", None);
    refused("input", None, "", None);
    refused("input", Some("{}"), "", Some(("tab", "t1")));
    refused("probe", None, "check", Some(("tab", "t1")));
    refused("probe", Some("{}"), "check", None);
    refused("input", Some("{}"), "check", None);
    refused("event", None, "", None);

    version += 1;
    insert(version, "event", Some("{}"), "", None).unwrap();
    let kinds: Vec<String> = conn
        .prepare("SELECT source_kind FROM blueprint_definitions ORDER BY source_kind")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(kinds, ["event"]);
}

/// The definitions table a database created before the source pair carries.
const PRE_SOURCE_DEFINITIONS_TABLE: &str = r#"
CREATE TABLE sentinel_definitions (
    sentinel_id TEXT NOT NULL REFERENCES sentinels(id),
    version INTEGER NOT NULL CHECK (version > 0),
    provenance TEXT NOT NULL CHECK (provenance IN ('authored', 'imported')),
    name TEXT,
    probe TEXT NOT NULL,
    observe_kind TEXT CHECK (observe_kind IN ('tab', 'cron_run')),
    observe_ref TEXT CHECK (observe_ref IS NULL OR length(observe_ref) > 0),
    question TEXT NOT NULL,
    interval_ms INTEGER NOT NULL CHECK (interval_ms > 0),
    until_bucket TEXT,
    escalation_timeout_ms INTEGER
        CHECK (escalation_timeout_ms IS NULL OR escalation_timeout_ms > 0),
    max_transitions INTEGER NOT NULL CHECK (max_transitions BETWEEN 1 AND 65535),
    max_actions INTEGER NOT NULL CHECK (max_actions BETWEEN 1 AND 65535),
    replaced_version INTEGER,
    author_tab_id TEXT CHECK (author_tab_id IS NULL OR length(author_tab_id) > 0),
    cause_transition_id TEXT
        CHECK (cause_transition_id IS NULL OR length(cause_transition_id) > 0),
    authored_at INTEGER CHECK (authored_at IS NULL OR authored_at >= 0),
    created_revision INTEGER NOT NULL CHECK (created_revision > 0),
    PRIMARY KEY (sentinel_id, version),
    CHECK ((observe_kind IS NULL) = (observe_ref IS NULL)),
    CHECK ((observe_kind IS NULL) = (length(probe) > 0)),
    CHECK (replaced_version IS NULL OR (replaced_version > 0 AND replaced_version < version)),
    CHECK ((replaced_version IS NULL) = (author_tab_id IS NULL)
        AND (replaced_version IS NULL) = (authored_at IS NULL)),
    CHECK (cause_transition_id IS NULL OR replaced_version IS NOT NULL)
) STRICT"#;

#[test]
fn sentinel_source_schema_migration_preserves_probe_and_observe_rows() {
    let root = TempRoot::new();
    Storage::create_at_schema_version(root.path(), 7).expect("previous schema fixture");
    {
        let conn = raw(root.path());
        conn.execute_batch(&format!(
            r#"
            DROP TABLE sentinel_definitions;
            {PRE_SOURCE_DEFINITIONS_TABLE};
            INSERT INTO sentinels (id, ordinal, creator_tab_id, workspace_id, team_id, cwd,
                agent_authored, scenario_id, revision) VALUES
                ('sentinel_probe_row', 0, 'tab-author', 'ws-alpha', NULL, 'C:/work', 0, NULL, 1),
                ('sentinel_observe_row', 1, 'tab-author', 'ws-alpha', NULL, 'C:/work', 0, NULL, 1);
            INSERT INTO sentinel_definitions (sentinel_id, version, provenance, name, probe,
                observe_kind, observe_ref, question, interval_ms, until_bucket,
                escalation_timeout_ms, max_transitions, max_actions, replaced_version,
                author_tab_id, cause_transition_id, authored_at, created_revision) VALUES
                ('sentinel_probe_row', 1, 'authored', 'probe-watch', 'git status --porcelain',
                    NULL, NULL, 'is the tree clean?', 20000, NULL, NULL, 8, 16,
                    NULL, NULL, NULL, NULL, 1),
                ('sentinel_observe_row', 1, 'authored', 'observe-watch', '',
                    'tab', 'tab-bound', 'what is the tab doing?', 30000, NULL, NULL, 4, 8,
                    NULL, NULL, NULL, NULL, 1);
            INSERT INTO sentinel_heads (sentinel_id, version) VALUES
                ('sentinel_probe_row', 1), ('sentinel_observe_row', 1);
            INSERT INTO sentinel_buckets (sentinel_id, version, ordinal, bucket, match_pattern,
                rearm_on_change) VALUES
                ('sentinel_probe_row', 1, 0, 'clean', '^clean$', 0),
                ('sentinel_observe_row', 1, 0, 'waiting', '^waiting$', 0);
            INSERT INTO sentinel_runtime (sentinel_id, generation, next_run, expires_at, paused,
                stopped, last_hash, last_bucket, last_reason, held, run_id, run_tab_id,
                run_bound_at, run_transitions) VALUES
                ('sentinel_probe_row', 3, 1000, NULL, 0, 0, NULL, NULL, NULL, NULL, NULL, NULL,
                    NULL, 0),
                ('sentinel_observe_row', 5, 2000, NULL, 0, 0, NULL, NULL, NULL, NULL, NULL, NULL,
                    NULL, 0);
            "#
        ))
        .expect("fixture rows");
        conn.close().unwrap();
    }

    let storage = Storage::open(root.path(), StorageConfig::default()).expect("migrates in place");
    let repo = storage.sentinels();
    let probe = repo
        .get(id("sentinel_probe_row"))
        .wait(Duration::from_secs(5))
        .unwrap()
        .value
        .expect("probe row survived");
    assert_eq!(
        probe.record.definition.definition.source,
        StageSource::Probe("git status --porcelain".into())
    );
    assert_eq!(
        probe.record.definition.definition.name.as_deref(),
        Some("probe-watch")
    );
    assert_eq!(
        probe.record.definition.definition.question,
        "is the tree clean?"
    );
    assert_eq!(probe.record.definition.definition.interval_ms, 20_000);
    assert_eq!(probe.record.definition.definition.buckets.len(), 1);
    assert_eq!(probe.record.runtime.generation, 3);
    assert_eq!(probe.record.runtime.next_run, 1_000);

    let observe = repo
        .get(id("sentinel_observe_row"))
        .wait(Duration::from_secs(5))
        .unwrap()
        .value
        .expect("observe row survived");
    assert_eq!(
        observe.record.definition.definition.source,
        StageSource::Observe(ObserveSource {
            kind: ObserveKind::Tab,
            reference: "tab-bound".into(),
        })
    );
    assert_eq!(observe.record.definition.definition.interval_ms, 30_000);
    assert_eq!(observe.record.runtime.generation, 5);

    let kinds: Vec<String> = raw(root.path())
        .prepare("SELECT source_kind FROM blueprint_definitions ORDER BY blueprint_id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(kinds, ["observe", "probe"]);
}

#[test]
fn blueprint_stage_source_storage_codec_round_trips_all_variants() {
    let (_root, storage) = storage();
    let pipeline = BlueprintSpec {
        name: "codec pipeline".into(),
        owner: BlueprintOwner::Tab,
        trigger: Trigger::Schedule {
            every: "20s".into(),
        },
        steps: vec![],
        result: BlueprintResult::default(),
    };
    let sources = [
        StageSource::Probe("status".into()),
        StageSource::Observe(ObserveSource {
            kind: ObserveKind::Tab,
            reference: "tab".into(),
        }),
        StageSource::Input(InputSpec {
            kind: MediaKind::Text,
            data: DataRef::Text("text".into()),
            convert_to: None,
            convert_timeout_ms: None,
        }),
        StageSource::Terminal {
            binding: ObserveSource {
                kind: ObserveKind::Tab,
                reference: "tab".into(),
            },
            config: codeterm_types::blueprint_terminal::Config {
                pattern: Some("ready".into()),
                idle_ms: None,
            },
        },
        StageSource::Pipeline(pipeline),
    ];
    for (index, source) in sources.into_iter().enumerate() {
        let key = format!("blueprint_codec_{index}");
        storage
            .sentinels()
            .create(SentinelCreate {
                identity: identity(&key),
                scenario_id: None,
                definition: definition(source.clone()),
                runtime: runtime(),
                associate: vec![],
            })
            .wait(Duration::from_secs(5))
            .unwrap();
        let stored = storage
            .sentinels()
            .get(id(&key))
            .wait(Duration::from_secs(5))
            .unwrap()
            .value
            .unwrap();
        assert_eq!(stored.record.definition.definition.source, source);
    }
}
