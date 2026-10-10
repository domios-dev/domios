use serde_json::{json, Value};

use crate::local_api::{LocalApiError, LocalApiResponse};
use crate::SurfaceAction;

#[derive(Debug, PartialEq)]
enum Outcome {
    Ok(Value),
    Refused { status: u16, body: String },
    Failed(String),
}

pub fn handle_surface(json: bool, action: SurfaceAction) {
    let (base, _) = crate::read_api_config();
    let http = crate::build_local_api().unwrap_or_else(|error| {
        crate::cmd::print::fail(1, format!("failed to connect to local API: {error}"));
    });
    let caller = std::env::var("CODETERM_TAB_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let outcome = match &action {
        SurfaceAction::Open {
            target,
            owner,
            title_from_owner,
        } => classify(
            http.post(format!("{base}/api/surfaces/open"))
                .json(&open_body(
                    target,
                    owner.as_deref(),
                    *title_from_owner,
                    caller.as_deref(),
                ))
                .send(),
        ),
        SurfaceAction::List => classify(http.get(format!("{base}/api/surfaces")).send()),
        SurfaceAction::Close { tab } => classify(
            http.post(format!("{base}/api/surfaces/close"))
                .json(&close_body(tab, caller.as_deref()))
                .send(),
        ),
    };
    match outcome {
        Outcome::Ok(value) if json => println!("{value}"),
        Outcome::Ok(value) => {
            for line in human_lines(&action, &value) {
                println!("{line}");
            }
        }
        Outcome::Refused { status, body } => crate::cmd::print::fail_http(1, status, &body),
        Outcome::Failed(message) => crate::cmd::print::fail(1, message),
    }
}

fn classify(result: Result<LocalApiResponse, LocalApiError>) -> Outcome {
    match result {
        Ok(response) if response.status().is_success() => {
            Outcome::Ok(response.json().unwrap_or_default())
        }
        Ok(response) => Outcome::Refused {
            status: response.status().as_u16(),
            body: response.text().unwrap_or_default(),
        },
        Err(error) => Outcome::Failed(error.to_string()),
    }
}

fn open_body(
    target: &str,
    owner: Option<&str>,
    title_from_owner: bool,
    caller: Option<&str>,
) -> Value {
    json!({
        "target": target,
        "owner_tab_id": owner,
        "caller_tab_id": caller,
        "title_from_owner": title_from_owner,
    })
}

fn close_body(tab: &str, caller: Option<&str>) -> Value {
    json!({ "tab_id": tab, "caller_tab_id": caller })
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("-")
}

fn human_lines(action: &SurfaceAction, value: &Value) -> Vec<String> {
    match action {
        SurfaceAction::Open { .. } => vec![format!(
            "Opened {}:{} as Tab {} ({}), owner {}",
            text(value, "plugin_id"),
            text(value, "surface_id"),
            text(value, "tab_id"),
            text(value, "title"),
            text(value, "owner_tab_id"),
        )],
        SurfaceAction::Close { .. } => {
            let mut lines = vec![format!("Closed surface Tab {}", text(value, "tab_id"))];
            lines.extend(
                value
                    .get("warnings")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|warning| format!("warning: {warning}")),
            );
            lines
        }
        SurfaceAction::List => list_lines(value),
    }
}

fn list_lines(value: &Value) -> Vec<String> {
    let rows = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let declared = rows("declared");
    let open = rows("open");
    let mut lines = vec![format!("Declared surfaces ({})", declared.len())];
    lines.extend(declared.iter().map(|row| {
        format!(
            "  {}:{}  {}  renderer={}{}",
            text(row, "plugin_id"),
            text(row, "surface_id"),
            text(row, "title"),
            text(row, "renderer"),
            if row.get("agent_bound").and_then(Value::as_bool) == Some(true) {
                "  agent-bound"
            } else {
                ""
            },
        )
    }));
    lines.push(format!("Open surface tabs ({})", open.len()));
    lines.extend(open.iter().map(|row| {
        format!(
            "  {}  {}:{}  {}  owner {}",
            text(row, "tab_id"),
            text(row, "plugin_id"),
            text(row, "surface_id"),
            text(row, "title"),
            text(row, "owner_tab_id"),
        )
    }));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_open_body_carries_owner_caller_and_label_flag() {
        let body = open_body("dash:board", Some("owner-1"), true, Some("caller-1"));
        assert_eq!(body["target"], "dash:board");
        assert_eq!(body["owner_tab_id"], "owner-1");
        assert_eq!(body["caller_tab_id"], "caller-1");
        assert_eq!(body["title_from_owner"], true);
        let bare = open_body("dash:board", None, false, None);
        assert!(bare["owner_tab_id"].is_null());
        assert!(bare["caller_tab_id"].is_null());
    }

    #[test]
    fn surface_close_body_names_the_tab_and_caller() {
        let body = close_body("tab-1", Some("caller-1"));
        assert_eq!(body["tab_id"], "tab-1");
        assert_eq!(body["caller_tab_id"], "caller-1");
    }

    #[test]
    fn surface_list_lines_count_declared_and_open_rows() {
        let value = json!({
            "declared": [{ "plugin_id": "dash", "surface_id": "board", "title": "Board", "renderer": "view", "agent_bound": true }],
            "open": [
                { "tab_id": "t1", "plugin_id": "dash", "surface_id": "board", "title": "Board", "owner_tab_id": "o1" },
                { "tab_id": "t2", "plugin_id": "dash", "surface_id": "board", "title": "Board", "owner_tab_id": null }
            ]
        });
        let lines = list_lines(&value);
        assert_eq!(lines.len(), 1 + 1 + 1 + 2);
        assert!(lines[1].contains("dash:board"));
        assert!(lines[3].contains("t1"));
    }

    #[test]
    fn surface_close_lines_surface_daemon_warnings() {
        let value = json!({ "tab_id": "t1", "warnings": ["w1", "w2"] });
        let lines = human_lines(&SurfaceAction::Close { tab: "t1".into() }, &value);
        assert_eq!(lines.len(), 3);
    }
}
