use axum::http::StatusCode;
use serde::Deserialize;

use crate::mem::layers::WakeUpOptions;

pub(crate) mod archive;

#[derive(Deserialize)]
pub struct ActivityRecentParams {
    #[serde(default)]
    pub since_ms: Option<u64>,
}

#[derive(Deserialize)]
pub struct PinnedListParams {
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Bumps `evidence_count` for L1-selectable kinds (skill|lesson|rule|fact|preference); no-op otherwise. Caller decides whether to invoke.
pub(crate) fn bump_on_get(conn: &rusqlite::Connection, kind: &str, id: &str) {
    if !matches!(kind, "skill" | "lesson" | "rule" | "fact" | "preference") {
        return;
    }
    if let Err(e) = codeterm_mem_store::graph::bump_evidence_count(conn, id) {
        log::debug!("[mem_get] evidence bump failed for {}: {}", id, e);
    }
}

pub(crate) fn save_schedules_extraction(
    outcome: &str,
    skip: bool,
    auto_extracted: bool,
    enabled: bool,
) -> bool {
    outcome != "unchanged" && !skip && !auto_extracted && enabled
}

#[derive(Deserialize)]
pub struct SaveBody {
    pub kind: String,
    #[serde(default)]
    pub scope: Option<String>,
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub body: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub importance: Option<f64>,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub skip_extract: bool,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub pinned: Option<bool>,
}

#[derive(Deserialize, Default)]
pub struct UpdateBody {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub importance: Option<f64>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub changed_by: Option<String>,
}

/// Shared field-level validation for a partial-update patch, used by both
/// the HTTP and socket transports so the two can never drift.
pub(crate) fn validate_update_body(body: &UpdateBody) -> Result<(), String> {
    if let Some(ref t) = body.title {
        let tt = t.trim();
        if tt.is_empty() || tt.len() < 3 || tt.len() > 200 {
            return Err("title must be 3..200 chars".into());
        }
    }
    if let Some(ref b) = body.body {
        if b.len() > 1_048_576 {
            return Err("body too large (max 1 MB)".into());
        }
    }
    if let Some(ref a) = body.area {
        if !a.is_empty()
            && (a.len() > 64
                || !a
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        {
            return Err(format!("invalid area: `{}`", a));
        }
    }
    if let Some(ref ts) = body.tags {
        if ts.len() > 20 {
            return Err("too many tags (max 20)".into());
        }
        for t in ts {
            if t.len() > 50 {
                return Err(format!("tag too long: `{}`", t));
            }
        }
    }
    if let Some(i) = body.importance {
        if !(0.0..=5.0).contains(&i) {
            return Err(format!("importance must be 0..5, got {}", i));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
pub(crate) struct WakeUpParams {
    #[serde(default)]
    pub budget: Option<usize>,
    #[serde(default)]
    pub l0_only: Option<bool>,
    #[serde(default)]
    pub no_l0: Option<bool>,
    #[serde(default)]
    pub explain: Option<bool>,
    #[serde(default)]
    pub workspace: Option<String>,
}

pub(crate) fn wake_up_options_from_params(
    default_l1_budget_tokens: usize,
    p: &WakeUpParams,
    caller_workspace: Option<String>,
) -> WakeUpOptions {
    let mut opts = WakeUpOptions::default();
    opts.include_l0 = !p.no_l0.unwrap_or(false);
    opts.include_l1 = !p.l0_only.unwrap_or(false);
    opts.explain = p.explain.unwrap_or(false);
    opts.current_workspace = p.workspace.clone().or(caller_workspace);

    if let Some(total) = p.budget {
        let total = total.max(1);
        match (opts.include_l0, opts.include_l1) {
            (true, true) => {
                let l0 = (total / 8).clamp(50, 100).min(total);
                opts.l0_budget_tokens = l0;
                opts.l1_budget_tokens = total.saturating_sub(l0);
            }
            (true, false) => {
                opts.l0_budget_tokens = total;
                opts.l1_budget_tokens = 0;
            }
            (false, true) => {
                opts.l0_budget_tokens = 0;
                opts.l1_budget_tokens = total;
            }
            (false, false) => {
                opts.l0_budget_tokens = 0;
                opts.l1_budget_tokens = 0;
            }
        }
    } else {
        opts.l1_budget_tokens = default_l1_budget_tokens;
    }

    opts
}

#[derive(Deserialize)]
pub struct ReembedParams {
    /// items | entities | all (default).
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Deserialize)]
pub struct LearnBody {
    pub text: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Deserialize)]
pub struct CheckDuplicateBody {
    pub body: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub threshold: Option<f64>,
}

/// Join an existing body and appended text with a blank-line separator.
/// Empty/whitespace-only bodies are replaced outright (no leading blank lines).
pub(crate) fn compose_appended_body(old: &str, text: &str) -> String {
    if old.trim().is_empty() {
        text.to_string()
    } else {
        format!("{}\n\n{}", old.trim_end_matches('\n'), text)
    }
}

pub(crate) fn pick_restore_body(
    history: &[crate::mem::store::HistoryEntry],
    version: Option<i64>,
) -> Result<String, (StatusCode, String)> {
    codeterm_mem::curation::pick_restore_body(history, version).map_err(|(code, msg)| {
        (
            StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST),
            msg,
        )
    })
}

#[derive(Deserialize)]
pub struct AppendBody {
    pub text: String,
    #[serde(default)]
    pub changed_by: Option<String>,
}

#[derive(Deserialize)]
pub struct RestoreBody {
    #[serde(default)]
    pub version: Option<i64>,
    #[serde(default)]
    pub changed_by: Option<String>,
}

#[derive(Deserialize)]
pub struct TaxonomyParams {
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Deserialize)]
pub struct TailParams {
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[cfg(test)]
mod tests;
