mod calibration;
mod decision;
mod decision_render;
#[cfg(test)]
mod decision_tests;
mod graph;
pub(crate) mod item;
mod skill;
mod socket;
mod tail;
pub(crate) mod transport;
#[cfg(test)]
mod transport_audit;
mod worker;

use crate::client::SocketClient;
use crate::cmd::print::handle_result;
use crate::{MemAction, MemSkillAction};

pub fn handle_mem(client: &mut SocketClient, json_mode: bool, action: MemAction) {
    let human_search = !json_mode && matches!(&action, MemAction::Search { .. });
    if let Some(result) = socket::try_dispatch(client, &action) {
        if human_search {
            match result {
                Ok(value) => print!("{}", render_search_hits(&value)),
                Err(error) => {
                    crate::cmd::print::fail(1, error.to_string());
                }
            }
            return;
        }
        print_mem_result(json_mode, result);
        return;
    }

    // Secrets retain authenticated HTTP; every remaining live action uses this socket client.
    match action {
        MemAction::History { id } => {
            let result = match item::handle_mem_history(client, id, json_mode) {
                Ok(None) => return,
                Ok(Some(v)) => Ok(v),
                Err(e) => Err(e),
            };
            handle_result(json_mode, result);
        }
        MemAction::Taxonomy { workspace } => {
            let result = match item::handle_mem_taxonomy(client, workspace, json_mode) {
                Ok(None) => return,
                Ok(Some(v)) => Ok(v),
                Err(e) => Err(e),
            };
            handle_result(json_mode, result);
        }
        MemAction::Tail {
            follow,
            limit,
            since,
        } => {
            tail::run_mem_tail(client, json_mode, follow, limit, since);
        }
        MemAction::Skill { action } => {
            let human_list = wants_human_skill_list(json_mode, &action);
            let result = skill::handle_mem_skill(client, action);
            if human_list {
                match result {
                    Ok(value) => print!("{}", render_mem_skill_list(&value)),
                    Err(error) => {
                        crate::cmd::print::fail(1, error.to_string());
                    }
                }
            } else {
                print_mem_result(json_mode, result);
            }
        }
        MemAction::Graph { action } => {
            print_mem_result(json_mode, graph::handle_mem_graph(client, action));
        }
        MemAction::Worker { action } => {
            print_mem_result(json_mode, worker::handle_mem_worker(client, action));
        }
        MemAction::Calibration { action } => {
            print_mem_result(
                json_mode,
                calibration::handle_mem_calibration(client, action),
            );
        }
        MemAction::Secret { action } => {
            crate::cmd::mem_secret::handle_mem_secret(json_mode, action);
        }
        MemAction::Decision { action } => decision::handle_mem_decision(json_mode, action),
        MemAction::RunWorker { .. } => unreachable!("RunWorker is short-circuited in main()"),
        MemAction::Dream { .. } => unreachable!("Dream is short-circuited in main()"),
        _ => unreachable!("all other mem actions have socket transport parity"),
    }
}

const SEARCH_PREVIEW_CHARS: usize = 120;

pub(crate) fn render_search_hits(value: &serde_json::Value) -> String {
    let Some(hits) = value
        .get("hits")
        .and_then(serde_json::Value::as_array)
        .or_else(|| value.as_array())
    else {
        return empty_render(value);
    };
    if hits.is_empty() {
        return empty_render(value);
    }
    let mut out = String::new();
    for hit in hits {
        if hit.get("source").and_then(serde_json::Value::as_str) == Some("session") {
            let provider = hit
                .get("provider")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?");
            let title = hit
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Untitled session");
            let pointer = hit
                .get("pointer")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("codeterm sessions get");
            let excerpt = hit
                .get("excerpt")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let normalized = excerpt.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut preview: String = normalized.chars().take(SEARCH_PREVIEW_CHARS).collect();
            if normalized.chars().count() > SEARCH_PREVIEW_CHARS {
                preview.push('…');
            }
            match decision_render::relevance_column(hit) {
                Some(relevance) => out.push_str(&format!(
                    "session [{provider}] {title} | {pointer} | {relevance} | {preview}\n"
                )),
                None => out.push_str(&format!(
                    "session [{provider}] {title} | {pointer} | {preview}\n"
                )),
            }
            continue;
        }
        if hit.get("source").and_then(serde_json::Value::as_str) == Some("doc") {
            let doc = hit
                .get("doc")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?");
            let section = hit
                .get("section")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?");
            let pointer = hit
                .get("pointer")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("codeterm docs");
            match decision_render::relevance_column(hit) {
                Some(relevance) => {
                    out.push_str(&format!("{doc} § {section} | {pointer} | {relevance}\n"))
                }
                None => out.push_str(&format!("{doc} § {section} | {pointer}\n")),
            }
            continue;
        }
        let item = hit.get("item").unwrap_or(hit);
        let text = item
            .get("summary")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| item.get("body").and_then(|value| value.as_str()))
            .unwrap_or("");
        let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut preview: String = normalized.chars().take(SEARCH_PREVIEW_CHARS).collect();
        if normalized.chars().count() > SEARCH_PREVIEW_CHARS {
            preview.push('…');
        }
        let id = item
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("?");
        let title = item
            .get("title")
            .and_then(|value| value.as_str())
            .unwrap_or("untitled");
        let kind = item
            .get("kind")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        let score = hit
            .get("score")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0);
        match decision_render::relevance_column(hit) {
            Some(relevance) => out.push_str(&format!(
                "{id} | {title} | {kind} | {score:.3} | {relevance} | {preview}\n"
            )),
            None => out.push_str(&format!(
                "{id} | {title} | {kind} | {score:.3} | {preview}\n"
            )),
        }
    }
    append_envelope_notice(&mut out, value);
    out
}

fn empty_render(response: &serde_json::Value) -> String {
    let mut out = String::from("(empty)\n");
    append_envelope_notice(&mut out, response);
    out
}

fn append_envelope_notice(out: &mut String, response: &serde_json::Value) {
    if let Some(notice) = decision_render::envelope_notice(response) {
        out.push_str(&notice);
        out.push('\n');
    }
}

/// codeterm-releases#60: one row per skill in columns — never the raw
/// `{"..."` dump `print_object`'s array-of-objects fallback produced.
/// `--json` always bypasses the column renderer, regardless of action shape.
fn wants_human_skill_list(json_mode: bool, action: &MemSkillAction) -> bool {
    !json_mode && matches!(action, MemSkillAction::List)
}

fn render_mem_skill_list(value: &serde_json::Value) -> String {
    let Some(rows) = value.get("skills").and_then(serde_json::Value::as_array) else {
        return "(no skills)\n".into();
    };
    if rows.is_empty() {
        return "(no skills)\n".into();
    }
    let mut out = String::new();
    for row in rows {
        let id = row.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let title = row
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("untitled");
        let area = row.get("area").and_then(|v| v.as_str()).unwrap_or("-");
        let updated_at = row
            .get("updated_at")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        out.push_str(&format!("{id} | {title} | {area} | {updated_at}\n"));
    }
    out
}

/// Print direct-injection text without the generic object wrapper.
fn print_mem_result(json_mode: bool, result: Result<serde_json::Value, String>) {
    if let Ok(ref val) = result {
        if !json_mode {
            if let (Some(outcome), Some(id)) = (
                val.get("outcome").and_then(|v| v.as_str()),
                val.get("id").and_then(|v| v.as_str()),
            ) {
                println!("{outcome}: {id}");
                return;
            }
            if let Some(id) = val.get("unchanged").and_then(|v| v.as_str()) {
                println!("unchanged: {id}");
                return;
            }
        }
        if !json_mode {
            if let Some(text) = val.get("text").and_then(|v: &serde_json::Value| v.as_str()) {
                println!("{}", text);
                return;
            }
        }
    }
    handle_result(json_mode, result);
}

#[cfg(test)]
mod tests;
