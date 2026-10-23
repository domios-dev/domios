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
                ReembedState::Completed | ReembedState::Failed | ReembedState::Stalled
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
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../evals/blueprint-recall");
    let frozen: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("config.frozen.json")).unwrap()).unwrap();
    let config: RecallConfig = serde_json::from_value(frozen["config"].clone()).unwrap();
    let controlled = "schema_version: 1\nname: credential-isolation\nnote: Check pipeline command credential isolation.\npipeline:\n  trigger: {type: schedule, every: 1d}\n  steps:\n    - {name: check, input: text, run: 'printf isolated'}\n  result: {to: notify}\n";
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
    for path in paths {
        let document = parse_document(&std::fs::read_to_string(path).unwrap()).unwrap();
        let intents = vec![document
            .note
            .clone()
            .unwrap_or_else(|| document.name.clone())];
        let id = fx
            .mem
            .save_blueprint_without_merge("probe", document, &intents, evidence.clone())
            .unwrap()
            .id;
        assert_eq!(fx.mem.index_blueprint(&id).unwrap().missing_vectors, 0);
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
        serde_json::json!({"scope":"controlled corpus with note-derived competitor intents; not BLACKSTAR snapshot", "target_yaml":if std::env::var("BLU45_SOLUTION_YAML").is_ok(){"provided"}else{"synthetic fixture"},"model":fx.embedder.identity(),"config":"frozen C10","intent":when,"query":query,"intent_cosine":cosine,"recalled_at_5":recalled,"top5":before,"enrichment":enrichment})
    );
}
