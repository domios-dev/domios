//! REM phase: prepare candidate data for the dreaming agent and spawn the
//! headless REM worker (dedup, classify, insights, diary).

use crate::{Entity, MemManager};

/// Prepare data for the REM agent — returns a text summary of candidates.
/// Called by `domios mem dream --report` and injected into the REM supervisor preface.
pub fn prepare_rem_data(mem: &MemManager) -> Result<String, String> {
    let blueprint_merges = mem.dream_blueprints()?;
    mem.store.rem_snapshot(blueprint_merges)
}

/// Ensure the dedicated dreams anchor (`project_dreams`) exists as a real entity
/// so `dreaming-normalize` never invalidates dream diary `belongs_to` edges.
pub fn ensure_dreams_anchor(mem: &MemManager) -> Result<(), String> {
    mem.graph_create_entity(&Entity {
        id: codeterm_mem_store::hubs::DREAMS_ANCHOR_ID.into(),
        entity_type: "Project".into(),
        scope: "global".into(),
        properties: serde_json::json!({}),
    })
}

/// Format the server-side `nightly_calibration` report for the REM supervisor preface.
pub fn format_calibration_preface(calibration: Option<&serde_json::Value>) -> String {
    let Some(cal) = calibration else {
        return "CALIBRATION (supervisor result): unavailable".to_string();
    };
    let skipped = cal
        .get("skipped_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("none");
    let examined = cal.get("examined").and_then(|v| v.as_u64()).unwrap_or(0);
    let applied = cal.get("applied").and_then(|v| v.as_u64()).unwrap_or(0);
    let archived = cal.get("archived").and_then(|v| v.as_u64()).unwrap_or(0);
    let stale = cal.get("stale");
    let stale_skipped = stale
        .and_then(|s| s.get("skipped_reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("none");
    let stale_archived = stale
        .and_then(|s| s.get("archived"))
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    format!(
        "CALIBRATION (supervisor result):\n  weekly_sweep: skipped={skipped} examined={examined} applied={applied} archived={archived}\n  stale_archive: skipped={stale_skipped} archived={stale_archived}"
    )
}

/// Escape any closing candidate-data fence in untrusted memory text so a crafted
/// record title cannot break out of the delimited candidate-data block.
pub fn format_candidate_data(rem_data: Option<&str>) -> String {
    const CLOSE_TAG: &str = "</untrusted-candidate-data";
    let Some(trimmed) = rem_data.map(str::trim).filter(|s| !s.is_empty()) else {
        return "(none)".to_string();
    };
    let mut out = String::with_capacity(trimmed.len());
    let mut rest = trimmed;
    while let Some(pos) = rest
        .as_bytes()
        .windows(CLOSE_TAG.len())
        .position(|w| w.eq_ignore_ascii_case(CLOSE_TAG.as_bytes()))
    {
        out.push_str(&rest[..pos]);
        out.push_str("&lt;");
        out.push_str(&rest[pos + 1..pos + CLOSE_TAG.len()]);
        rest = &rest[pos + CLOSE_TAG.len()..];
    }
    out.push_str(rest);
    out
}

pub const DREAMING_TELEGRAM_STEP_ENABLED: &str = r#"Then push the INSIGHT (only the insight prose, not the log block) to
telegram so the vault talks back. OK if it errors — the diary in mem
is the durable artifact:

    domios mem search "telegram chat id" --limit 1
    # extract the chat id, then:
    domios bridge send --to <chat_id> --service telegram "🧠 Dreamer brief\n\n<just the insight, no <insight> tags, no log>"
    # If no fact found or bridge errors, skip — telegram is nice-to-have."#;

pub const DREAMING_TELEGRAM_STEP_DISABLED: &str = "Telegram notification is DISABLED \
    (memory.dreaming_telegram_notify=false). Skip any outbound notification — \
    do not search for chat ids, do not invoke the bridge command. The diary \
    in mem is the artifact; the user has opted out of the nightly Telegram ping.";

pub fn render_rem_prompt(
    started_at: chrono::DateTime<chrono::Utc>,
    telegram_step: &str,
    supervisor_preface: &str,
    candidate_data: &str,
) -> String {
    // Substitute host-controlled tokens before supervisor preface and untrusted
    // candidate data so record titles containing placeholder tokens are never rewritten.
    let fenced_candidates = format_candidate_data(Some(candidate_data));
    crate::prompt_text::DREAMING_REM
        .replace(
            "{started_at}",
            &started_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        )
        .replace(
            "{calibration_since}",
            &(started_at - chrono::Duration::hours(1))
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        )
        .replace(
            "{diary_title}",
            &format!("dream: {}", started_at.format("%Y-%m-%d %H:%M")),
        )
        .replace("{telegram_notify_step}", telegram_step)
        .replace("{supervisor_preface}", supervisor_preface)
        .replace("{candidate_data}", &fenced_candidates)
}
