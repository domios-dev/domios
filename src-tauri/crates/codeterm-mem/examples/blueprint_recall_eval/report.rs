use crate::confirm::Results;
use crate::evaluate::{Dedupe, Recall};
use crate::fixtures::{self, eval_dir};
use crate::grid::{self, Grid};

fn cell(recall: &Recall) -> String {
    format!("{:.3}/{:.3}/{:.3}", recall.r1, recall.r3, recall.r5)
}

fn dedupe_line(label: &str, dedupe: &Dedupe) -> String {
    format!(
        "| {label} | {} | {:.4} | {} |",
        dedupe.pairs, dedupe.accuracy, dedupe.exact_or_structural_on_distinct
    )
}

pub fn report() -> Result<(), String> {
    if let Ok(grid) = fixtures::read_json::<Grid>(&grid::grid_path()) {
        println!(
            "## Grid (selection pool; recall@1/3/5; embedder {})\n",
            grid.embedder
        );
        println!("| config | fusion | pool intents | weights i/w/s/c/l | pooled | set a | set b | set b2 |");
        println!("|---|---|---|---|---|---|---|---|");
        for row in &grid.rows {
            let w = &row.config.weights;
            let get = |key: &str| row.tables.get(key).map(cell).unwrap_or_default();
            println!(
                "| {}{} | {:?} | {} | {}/{}/{}/{}/{} | {} | {} | {} | {} |",
                row.id,
                if row.id == grid.selected {
                    " (selected)"
                } else {
                    ""
                },
                row.config.fusion,
                row.config.pool_intents,
                w.intent,
                w.worker_intent,
                w.summary,
                w.canon,
                w.lexical,
                get("pooled"),
                get("set:a"),
                get("set:b"),
                get("set:b2"),
            );
        }
        println!("\nselection: {} -> {}", grid.selection_key, grid.selected);
        println!(
            "enrichment applied to {} documents; refused by the product save path: {:?}",
            grid.enrichment_applied, grid.refused
        );
        println!(
            "dedupe thresholds (dev-calibrated): {:?}\n",
            grid.thresholds
        );
        println!("| dedupe | pairs | accuracy | exact/structural on distinct |\n|---|---|---|---|");
        println!("{}", dedupe_line("dev (calibration)", &grid.dedupe_dev));
        println!(
            "{}\n",
            dedupe_line(
                "test (informational, grid)",
                &grid.dedupe_test_informational
            )
        );
    }
    let mut files = std::fs::read_dir(eval_dir())
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    (name.starts_with("results-") || name.starts_with("posthoc-"))
                        && name.ends_with(".json")
                })
        })
        .collect::<Vec<_>>();
    files.sort();
    for path in files {
        let results: Results = fixtures::read_json(&path)?;
        println!(
            "## {} ({}; config {}; embedder {}; ran {}; head {})\n",
            results.label,
            if results.posthoc {
                "post-hoc"
            } else {
                "confirmation"
            },
            results.config_id,
            results.embedder,
            results.ran_at,
            results.head
        );
        println!(
            "PREREG {} | frozen {} | queries sha256 {} | manifest sha256 {}",
            results.prereg_sha,
            results.frozen_sha,
            results.queries_sha256,
            results.manifest_sha256.as_deref().unwrap_or("-")
        );
        println!(
            "enrichment applied to {} documents; refused by the product save path: {:?}\n",
            results.enrichment_applied, results.refused
        );
        println!("| arm | n | pooled recall@1/3/5 | starved originals @1/3/5 |\n|---|---|---|---|");
        for (name, arm) in [
            ("gate (C*, enrichment, no rerank)", &results.gate),
            ("degraded (C*, enrichment off)", &results.degraded),
            ("rerank (C* + worker_model rerank)", &results.rerank),
        ] {
            println!(
                "| {name} | {} | {} | {} |",
                arm.tables["pooled"].n,
                cell(&arm.tables["pooled"]),
                cell(&arm.starved)
            );
        }
        let applied = results
            .rerank_log
            .iter()
            .filter(|log| log.status == "applied")
            .count();
        let paused = results
            .rerank_log
            .iter()
            .filter(|log| log.status == "paused")
            .count();
        println!(
            "\nrerank applied on {applied} of {} queries ({paused} paused by the quota gate)",
            results.rerank_log.len()
        );
        println!("starved originals: {:?}\n", results.starved_originals);
        println!("| original | n | gate | degraded | rerank |\n|---|---|---|---|---|");
        for (original, gate) in &results.gate.per_original {
            println!(
                "| {original} | {} | {} | {} | {} |",
                gate.n,
                cell(gate),
                results
                    .degraded
                    .per_original
                    .get(original)
                    .map(cell)
                    .unwrap_or_default(),
                results
                    .rerank
                    .per_original
                    .get(original)
                    .map(cell)
                    .unwrap_or_default()
            );
        }
        println!(
            "\n| dedupe | pairs | accuracy | exact/structural on distinct |\n|---|---|---|---|"
        );
        println!(
            "{}",
            dedupe_line("E2 test pairs (frozen thresholds)", &results.dedupe)
        );
        println!(
            "\nGate B (recall@5 >= 0.80 on {}): {}\n",
            results.label, results.gate_b
        );
    }
    Ok(())
}
