use crate::evaluate::Hit;
use crate::generate::run_claude;
use crate::library::{Library, WORKER_MODEL};
use codeterm_mem::blueprint_library::BlueprintHit;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_CANDIDATES: usize = 10;

pub fn selection(scores: &[f64]) -> Vec<usize> {
    let mut order: Vec<_> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[a].total_cmp(&scores[b]).then(a.cmp(&b)));
    order.truncate(MAX_CANDIDATES);
    order
}

pub fn skip_reason(scores: &[f64]) -> Option<&'static str> {
    if scores.len() <= 3 {
        return Some("small_pool");
    }
    if scores.iter().any(|score| !score.is_finite()) {
        return None;
    }
    let order = selection(scores);
    let top = scores[order[0]];
    let second = scores[order[1]];
    let gap = second - top;
    (top.is_finite()
        && second.is_finite()
        && gap >= 0.25
        && gap >= top.abs().max(second.abs()) * 0.5)
        .then_some("decisive_ranking")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RerankLog {
    pub status: String,
    pub candidates: Vec<String>,
    pub order: Vec<String>,
    pub raw: Value,
}

fn describe(library: &Library, hit: &BlueprintHit, label: &str) -> String {
    let document = &hit.entry.document;
    let enrichment = library
        .mem
        .blueprint_index(&hit.entry.id)
        .ok()
        .flatten()
        .and_then(|row| row.current_enrichment().map(|e| e.summary.clone()));
    format!(
        "- id: {label}\n  name: {}\n  note: {}\n  intents: {}\n  summary: {}",
        document.name,
        document.note.as_deref().unwrap_or(""),
        hit.entry.intents.join("; "),
        enrichment.unwrap_or_default()
    )
}

pub fn rerank(
    library: &Library,
    query: &str,
    fused: &[BlueprintHit],
    baseline: Vec<Hit>,
) -> (Vec<Hit>, RerankLog) {
    let scores = fused.iter().map(|hit| -hit.score).collect::<Vec<_>>();
    let selected = selection(&scores);
    let labels = (1..=selected.len())
        .map(|n| format!("b{n}"))
        .collect::<Vec<_>>();
    let candidates = selected
        .iter()
        .map(|index| baseline[*index].id.clone())
        .collect::<Vec<_>>();
    let log = |status: &str, order: Vec<String>, raw: Value| RerankLog {
        status: status.into(),
        candidates: candidates.clone(),
        order,
        raw,
    };
    if let Some(reason) = skip_reason(&scores) {
        return (
            baseline,
            log(&format!("skipped:{reason}"), Vec::new(), Value::Null),
        );
    }
    let items = selected
        .iter()
        .zip(&labels)
        .map(|(index, label)| describe(library, &fused[*index], label))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = codeterm_mem::prompt_text::MEM_RERANK
        .replace("{query}", query)
        .replace("{items}", &items);
    if let Err(paused) = crate::quota::gate() {
        return (
            baseline,
            log("paused", Vec::new(), Value::String(paused.reason)),
        );
    }
    let raw = match run_claude(WORKER_MODEL, &prompt) {
        Ok(raw) => raw,
        Err(error) => return (baseline, log("failed", Vec::new(), Value::String(error))),
    };
    let text = raw["result"].as_str().unwrap_or_default();
    let parsed = text
        .find('[')
        .zip(text.rfind(']'))
        .and_then(|(start, end)| serde_json::from_str::<Vec<String>>(&text[start..=end]).ok());
    let Some(order) = parsed else {
        return (baseline, log("unparsed", Vec::new(), raw));
    };
    let mut ranked = Vec::new();
    for label in &order {
        if let Some(position) = labels.iter().position(|known| known == label) {
            let hit = baseline[selected[position]].clone();
            if !ranked.iter().any(|kept: &Hit| kept.id == hit.id) {
                ranked.push(hit);
            }
        }
    }
    for index in &selected {
        if !ranked
            .iter()
            .any(|kept: &Hit| kept.id == baseline[*index].id)
        {
            ranked.push(baseline[*index].clone());
        }
    }
    for hit in &baseline {
        if !ranked.iter().any(|kept: &Hit| kept.id == hit.id) {
            ranked.push(hit.clone());
        }
    }
    let order_ids = ranked.iter().map(|hit| hit.id.clone()).collect();
    (ranked, log("applied", order_ids, raw))
}
