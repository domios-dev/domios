use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crate::blueprint_library::RecallConfig;
use crate::embedder::local::{default_model, LocalProvider};
use crate::embedder::testing::{fixture_for, note, MockProvider};
use crate::embedder::{EmbedderEvent, ReembedState};

#[test]
#[ignore = "manual real-L12 throughput measurement; run debug and optimized on an authorized host"]
fn recall_reembed_measure_real_l12_1k() {
    let fx = fixture_for(MockProvider::new("fixture", "old", 384));
    for item in 0..1000 {
        crate::store::insert(&fx.mem.conn.lock(), &note(&format!("Fixture {item}"), "Check pipeline command isolation and preserve private credentials while chaining independent tasks."), &fx.mem.blobs_dir).unwrap();
    }
    let (sender, events) = mpsc::channel();
    fx.embedder.subscribe(Arc::new(move |event| {
        let _ = sender.send(event.clone());
    }));
    let started = Instant::now();
    fx.mem
        .switch_embedder(Arc::new(LocalProvider::new(default_model())))
        .unwrap();
    let deadline = started + Duration::from_secs(1200);
    let progress = loop {
        let event = events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("1k fixture exceeded its 20-minute bound");
        if let EmbedderEvent::Progress(progress) = event {
            if matches!(
                progress.state,
                ReembedState::Completed | ReembedState::Failed
            ) {
                break progress;
            }
        }
    };
    let elapsed = started.elapsed().as_secs_f64();
    println!(
        "{}",
        serde_json::json!({"fixture":"1k short ordinary mem items; cold provider including load and publication","debug_assertions":cfg!(debug_assertions),"items":1000,"seconds":elapsed,"items_per_second":1000.0/elapsed,"progress":progress})
    );
    assert_eq!(
        progress.state,
        ReembedState::Completed,
        "{:?}",
        progress.last_error
    );
    assert_eq!(progress.done, 1000);
}

#[test]
#[ignore = "manual real-L12/C10 controlled pair evaluation; original BLACKSTAR corpus is not implied"]
fn recall_reembed_measure_real_l12_c10_pair() {
    use codeterm_blueprint_types::blueprint_memory::{parse_document, Evidence};
    let fx = fixture_for(Arc::new(LocalProvider::new(default_model())));
    let root = crate::test_support::fixture_manifest_dir().join("../../../evals/blueprint-recall");
    let frozen: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("config.frozen.json")).unwrap()).unwrap();
    let config: RecallConfig = serde_json::from_value(frozen["config"].clone()).unwrap();
    let controlled = v2_watch(
        "credential-isolation",
        "Check pipeline command credential isolation.",
    );
    let source = std::env::var("BLU45_SOLUTION_YAML")
        .ok()
        .map(|path| std::fs::read_to_string(path).unwrap())
        .unwrap_or_else(|| controlled.into());
    let when = "check that pipeline commands cannot see Domios credentials";
    let query = "ensure chained tasks never read tokens";
    let evidence = Evidence {
        kept: true,
        successes: 1,
        ..Default::default()
    };
    let target = fx
        .mem
        .save_blueprint_without_merge(
            "probe",
            parse_document(&source).unwrap(),
            &[when.into()],
            evidence.clone(),
        )
        .unwrap()
        .id;
    assert_eq!(fx.mem.index_blueprint(&target).unwrap().missing_vectors, 0);
    let mut paths = std::fs::read_dir(root.join("corpus"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "yaml")
        })
        .collect::<Vec<_>>();
    paths.sort();
    let mut competitors = 0;
    for path in paths {
        let text = std::fs::read_to_string(path).unwrap();
        let field = |key: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(key))
                .map(|value| value.trim().trim_matches('\'').to_string())
        };
        let (Some(name), Some(note)) = (field("name:"), field("note:")) else {
            continue;
        };
        let slug = name
            .to_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let id = fx
            .mem
            .save_blueprint_without_merge(
                "probe",
                parse_document(&v2_watch(&slug, &note)).unwrap(),
                &[note],
                evidence.clone(),
            )
            .unwrap()
            .id;
        assert_eq!(fx.mem.index_blueprint(&id).unwrap().missing_vectors, 0);
        competitors += 1;
    }
    let hits = fx
        .mem
        .recall_blueprint_hits("probe", query, None, 5, &config)
        .unwrap();
    let recalled = hits.iter().any(|hit| hit.entry.id == target);
    let pair = fx.embedder.passage(when).unwrap();
    let vector = fx.embedder.query(query).unwrap();
    let cosine = crate::blueprint_library::index::cosine(&pair.vector, &vector.vector);
    let before = hits
        .iter()
        .map(|hit| serde_json::json!({"id":hit.entry.id,"score":hit.score,"why":hit.why}))
        .collect::<Vec<_>>();
    let enrichment = if let Ok(path) = std::env::var("BLU45_ENRICHMENT_REPORT") {
        let hash = fx
            .mem
            .blueprint_index(&target)
            .unwrap()
            .unwrap()
            .document_hash;
        fx.mem
            .complete_blueprint_enrichment(
                &target,
                &hash,
                "recorded-save-time-worker",
                &std::fs::read_to_string(path).unwrap(),
            )
            .unwrap();
        let hits = fx
            .mem
            .recall_blueprint_hits("probe", query, None, 5, &config)
            .unwrap();
        serde_json::json!({"recalled_at_5":hits.iter().any(|hit|hit.entry.id==target),"provenance":"provided save-time report"})
    } else {
        serde_json::json!({"provenance":"save-time report not supplied; coverage unmeasured"})
    };
    println!(
        "{}",
        serde_json::json!({"scope":"controlled v2 fixture: one shared watch skeleton per document, competitor intents from evals/blueprint-recall corpus names and notes; not BLACKSTAR snapshot","competitors":competitors, "target_yaml":if std::env::var("BLU45_SOLUTION_YAML").is_ok(){"provided"}else{"synthetic fixture"},"model":fx.embedder.identity(),"config":"frozen C10","intent":when,"query":query,"intent_cosine":cosine,"recalled_at_5":recalled,"top5":before,"enrichment":enrichment})
    );
}

fn v2_watch(name: &str, note: &str) -> String {
    format!(
        "schema_version: 2
kind: watch
name: {name}
note: {note:?}
steps:
- id: source
  uses: probe
  with:
    command:
      lit: printf isolated
  every:
    lit: 1d
- id: decide
  uses: ask
  with:
    question:
      lit: done
  buckets:
  - done
  matches:
    done:
      lit: isolated
  until: done
  routes:
    done: alert
- id: alert
  uses: notify
"
    )
}
