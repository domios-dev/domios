//! Source contract: input definitions round-trip, the columns refuse zero or two sources,
//! and native v2 documents survive source storage round-trips.

use super::*;
use crate::assets::{self, Layout};
use crate::schema::open_existing;
use crate::{Storage, StorageConfig};
use codeterm_blueprint_types::blueprint_pipeline::BlueprintSpec;
use codeterm_blueprint_types::sentinel_input::{DataRef, InputSpec, MediaKind};
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
        creation_options: None,
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
        transition_offset: 0,
        action_offset: 0,
        stopped_at: None,
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

fn watch(source: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"schema_version":2,"kind":"watch","name":"source-round-trip","steps":[
        source,
        {"id":"decide","uses":"ask","with":{"question":{"lit":"did it finish?"}},"buckets":["done"],"matches":{"done":{"lit":"^done$"}},"until":"done","routes":{"done":"@stop"}}
    ]})
}
fn pipeline_document(name: &str, owner: &str, target: &str) -> serde_json::Value {
    serde_json::json!({"schema_version":2,"kind":"pipeline","name":name,"owner":owner,"trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"emit","run":{"lit":"printf ready"}}],"result":{"to":target}})
}
fn pipeline(document: &serde_json::Value) -> BlueprintSpec {
    let runtime = codeterm_blueprint_types::blueprint_document::Document::from_definition(document)
        .unwrap()
        .lower()
        .unwrap();
    serde_json::from_value(runtime["spec"].clone()).unwrap()
}

fn definition(document: serde_json::Value, source: StageSource) -> StageDefinition {
    codeterm_blueprint_types::blueprint_document::Document::from_definition(&document)
        .unwrap()
        .lower()
        .unwrap();
    let pipeline = matches!(source, StageSource::Pipeline(_));
    let name = document["name"].as_str().unwrap().to_string();
    StageDefinition {
        document: Some(document),
        note: None,
        model_classification: false,
        session: false,
        name: Some(name),
        source,
        question: if pipeline {
            String::new()
        } else {
            "did it finish?".into()
        },
        interval_ms: 20_000,
        until: if pipeline { None } else { Some("done".into()) },
        escalation_timeout_ms: None,
        max_transitions: 8,
        max_actions: 16,
        buckets: if pipeline {
            Vec::new()
        } else {
            vec![BucketDefinition {
                name: "done".into(),
                match_pattern: Some("^done$".into()),
                rearm_on_change: false,
                handlers: Vec::new(),
            }]
        },
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
        definition: definition(watch(serde_json::json!({"id":"source","uses":"input","with":{"kind":{"lit":"audio"},"file":{"lit":std::env::current_dir().unwrap().join("voice.wav")},"convert_to":{"lit":"text"},"convert_timeout_ms":{"lit":30000}},"every":{"lit":"20s"},"timeout":{"lit":"1h"}})),StageSource::Input(input.clone())),
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
    let next = definition(
        watch(
            serde_json::json!({"id":"source","uses":"input","with":{"kind":{"lit":"audio"},"url":{"lit":"https://example.com/voice.wav"},"convert_to":{"lit":"text"}},"every":{"lit":"20s"},"timeout":{"lit":"1h"}}),
        ),
        StageSource::Input(converted.clone()),
    );
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
    let document = pipeline_document("echo pipeline", "team", "notify");
    let pipeline = pipeline(&document);
    let definition = definition(document, StageSource::Pipeline(pipeline.clone()));
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
#[test]
fn blueprint_stage_source_storage_codec_round_trips_all_variants() {
    let (_root, storage) = storage();
    let document = pipeline_document("codec pipeline", "tab", "owner");
    let sources = [
        (
            watch(
                serde_json::json!({"id":"source","uses":"probe","with":{"command":{"lit":"status"}},"timeout":{"lit":"1h"}}),
            ),
            StageSource::Probe("status".into()),
        ),
        (
            watch(
                serde_json::json!({"id":"source","uses":"observe","with":{"kind":{"lit":"tab"},"id":{"lit":"tab"}},"timeout":{"lit":"1h"}}),
            ),
            StageSource::Observe(ObserveSource {
                kind: ObserveKind::Tab,
                reference: "tab".into(),
            }),
        ),
        (
            watch(
                serde_json::json!({"id":"source","uses":"input","with":{"kind":{"lit":"text"},"text":{"lit":"text"}},"timeout":{"lit":"1h"}}),
            ),
            StageSource::Input(InputSpec {
                kind: MediaKind::Text,
                data: DataRef::Text("text".into()),
                convert_to: None,
                convert_timeout_ms: None,
            }),
        ),
        (
            watch(
                serde_json::json!({"id":"source","uses":"terminal","with":{"kind":{"lit":"tab"},"id":{"lit":"tab"},"pattern":{"lit":"ready"}},"timeout":{"lit":"1h"}}),
            ),
            StageSource::Terminal {
                binding: ObserveSource {
                    kind: ObserveKind::Tab,
                    reference: "tab".into(),
                },
                config: codeterm_blueprint_types::blueprint_terminal::Config {
                    pattern: Some("ready".into()),
                    idle_ms: None,
                },
            },
        ),
        (document.clone(), StageSource::Pipeline(pipeline(&document))),
    ];
    for (index, (mut document, mut source)) in sources.into_iter().enumerate() {
        let key = format!("blueprint_codec_{index}");
        let name = format!("source-round-trip-{index}");
        document["name"] = serde_json::json!(name);
        if let StageSource::Pipeline(spec) = &mut source {
            spec.name = name;
        }
        let definition = definition(document, source.clone());
        storage
            .sentinels()
            .create(SentinelCreate {
                identity: identity(&key),
                scenario_id: None,
                definition,
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
#[test]
fn blueprint_names_are_unique_per_team_workspace_and_options_persist() {
    let (_root, storage) = storage();
    let repo = storage.sentinels();
    let make = |id: &str, team: &str| {
        let mut identity = identity(id);
        identity.scope.team_id = Some(team.into());
        identity.creation_options =
            Some(codeterm_blueprint_types::blueprint_watch::CreationOptions {
                timeout_ms: Some(3600000),
                paused: true,
            });
        SentinelCreate {
            identity,
            scenario_id: None,
            definition: definition(
                watch(
                    serde_json::json!({"id":"source","uses":"probe","with":{"command":{"lit":"echo done"}},"timeout":{"lit":"1h"}}),
                ),
                StageSource::Probe("echo done".into()),
            ),
            runtime: runtime(),
            associate: Vec::new(),
        }
    };
    repo.create(make("blueprint_first", "team_a"))
        .wait(Duration::from_secs(5))
        .unwrap();
    assert!(repo
        .create(make("blueprint_duplicate", "team_a"))
        .wait(Duration::from_secs(5))
        .is_err());
    repo.create(make("blueprint_other_team", "team_b"))
        .wait(Duration::from_secs(5))
        .unwrap();
    let stored = repo
        .get(id("blueprint_first"))
        .wait(Duration::from_secs(5))
        .unwrap()
        .value
        .unwrap();
    assert_eq!(
        stored
            .record
            .identity
            .creation_options
            .as_ref()
            .unwrap()
            .timeout_ms,
        Some(3600000)
    );
    assert!(
        stored
            .record
            .identity
            .creation_options
            .as_ref()
            .unwrap()
            .paused
    );
}
