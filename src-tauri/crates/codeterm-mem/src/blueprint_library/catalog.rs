use codeterm_blueprint_types::blueprint_document::Document;

pub struct CatalogEntry {
    pub id: &'static str,
    pub source: &'static str,
}

macro_rules! entry {
    ($name:literal) => {
        CatalogEntry {
            id: concat!("catalog:", $name),
            source: include_str!(concat!(
                "../../resources/blueprint-catalog/",
                $name,
                ".yaml"
            )),
        }
    };
}

pub const ENTRIES: &[CatalogEntry] = &[
    entry!("checks-fan-in"),
    entry!("command-lines"),
    entry!("latency-percentiles"),
    entry!("log-dedupe"),
    entry!("rule-classifier"),
    entry!("severity-grade"),
];

pub const UNAVAILABLE: &[(&str, &str)] = &[
    ("catalog:github-pr-checks", "Native PR checks emits statusCheckRollup JSON, not the catalog checks array port"),
    ("catalog:github-run-jobs", "Native run/jobs includes source_state, failed_jobs and raw fields excluded by the catalog strict port"),
    ("catalog:github-runners", "Native runners emits state/online_count/label, not the catalog runners array port"),
    ("catalog:cron-run-wait", "Native callable observe has no cron-run wait/timeout result contract"),
    ("catalog:escalation-ladder", "Native callable ask has no argument-bound acknowledgement deadline contract"),
    ("catalog:http-probe", "Native HTTP response does not expose the catalog status/latency_ms/body port"),
    ("catalog:notify-digest", "Native notify has no typed bound-payload result contract"),
    ("catalog:prompt-responder", "Native provider-declared response has no catalog screening-result contract"),
    ("catalog:pattern-incident-triage", "Depends on unavailable http-probe, notify-digest and escalation-ladder contracts"),
    ("catalog:pattern-release-gate", "Depends on unavailable http-probe, cron-run-wait, notify-digest and escalation-ladder contracts"),
];

pub fn document(id: &str) -> Result<Document, String> {
    let Some(entry) = ENTRIES.iter().find(|entry| entry.id == id) else {
        let reason = UNAVAILABLE
            .iter()
            .find(|(key, _)| *key == id)
            .map_or("Unknown catalog id", |(_, reason)| *reason);
        return Err(format!("Catalog Blueprint unavailable: {id}: {reason}"));
    };
    let document: Document =
        serde_yaml::from_str(entry.source).map_err(|error| error.to_string())?;
    let errors = document.template_errors();
    if !errors.is_empty() {
        return Err(format!(
            "Catalog Blueprint is not native v2: {id}: {errors:?}"
        ));
    }
    Ok(document)
}

#[cfg(test)]
mod tests;
