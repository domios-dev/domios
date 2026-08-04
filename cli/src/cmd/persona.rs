//! `codeterm persona` — inspect the shared role preset resolver. The daemon
//! owns the text; this surface only asks for it and prints it verbatim.

use serde_json::{json, Value};

use crate::args::persona::PersonaAction;
use crate::client::SocketClient;

pub fn handle(
    client: &mut SocketClient,
    json_output: bool,
    action: PersonaAction,
) -> Result<Option<Value>, String> {
    match action {
        PersonaAction::List => catalog(client, json_output, "persona.list"),
        PersonaAction::Reload => catalog(client, json_output, "persona.reload"),
        PersonaAction::Show {
            role,
            kind,
            guidance,
            occasion,
        } => show(client, json_output, role, kind, guidance, occasion),
    }
}

fn catalog(
    client: &mut SocketClient,
    json_output: bool,
    method: &str,
) -> Result<Option<Value>, String> {
    let result = client.call(method, Some(json!({})))?;
    if json_output {
        return Ok(Some(result));
    }
    println!("{}", render_list(&result));
    Ok(None)
}

fn show(
    client: &mut SocketClient,
    json_output: bool,
    role: String,
    kind: Option<String>,
    guidance: Option<String>,
    occasion: Option<String>,
) -> Result<Option<Value>, String> {
    let mut params = json!({ "role": role });
    if let Some(kind) = kind {
        params["kind"] = json!(kind);
    }
    if let Some(guidance) = guidance {
        params["guidance"] = json!(guidance);
    }
    if let Some(occasion) = occasion {
        params["occasion"] = json!(occasion);
    }
    let result = client.call("persona.show", Some(params))?;
    if json_output {
        return Ok(Some(result));
    }
    println!("{}", render_show(&result));
    Ok(None)
}

fn occasions(view: &Value) -> Vec<Value> {
    view.get("occasions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn render_show(view: &Value) -> String {
    let mut out = format!(
        "{} ({})\nrole: {}\nlabel: {}\n\nfragments:\n",
        field(view, "kind_id"),
        field(view, "origin"),
        field(view, "role"),
        field(view, "label"),
    );
    let fragments = view
        .get("fragments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if fragments.is_empty() {
        out.push_str("  (none)\n");
    }
    for fragment in &fragments {
        let path = fragment
            .get("path")
            .and_then(Value::as_str)
            .map(|path| format!(" <- {path}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "  [{}]{} {} chars\n",
            field(fragment, "source"),
            path,
            field(fragment, "text").len()
        ));
    }
    for occasion in occasions(view) {
        out.push_str(&format!(
            "\n=== {} ({}) ===\n{}\n",
            field(&occasion, "occasion"),
            field(&occasion, "emission"),
            field(&occasion, "text"),
        ));
    }
    out
}

fn render_list(view: &Value) -> String {
    let mut out = format!("overlay dir: {}\n\nkinds:\n", field(view, "overlay_dir"));
    for kind in view
        .get("kinds")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        out.push_str(&format!(
            "  {:<16} {:<22} {:<12} anchors={} authority={} spawn={}\n",
            field(&kind, "kind_id"),
            field(&kind, "label"),
            field(&kind, "origin"),
            kind.get("anchors_team").and_then(Value::as_bool) == Some(true),
            kind.get("team_authority").and_then(Value::as_bool) == Some(true),
            kind.get("can_spawn").and_then(Value::as_bool) == Some(true),
        ));
    }
    let rejected = view
        .get("rejected")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !rejected.is_empty() {
        out.push_str("\nrejected overlays:\n");
        for rejection in &rejected {
            out.push_str(&format!(
                "  {} [{}] {}\n",
                field(rejection, "path"),
                field(rejection, "reason"),
                field(rejection, "detail"),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{render_list, render_show};

    fn view() -> serde_json::Value {
        json!({
            "role": "Worker",
            "kind_id": "rust-backend-dev",
            "label": "Rust Backend Dev",
            "origin": "overlay",
            "fragments": [
                {"source": "inherited_base", "text": "BASE"},
                {"source": "disk_overlay", "path": "role-presets/rust-backend-dev.toml", "text": "DISK"},
                {"source": "task_overlay", "text": "You are Mira."}
            ],
            "occasions": [
                {"occasion": "launch_assignment", "emission": "preset", "text": "BASE\n\nDISK"},
                {"occasion": "worker_launch_identity", "emission": "preset", "text": "You are Mira."}
            ]
        })
    }

    #[test]
    fn show_renders_every_fragment_provenance_and_occasion_text_verbatim() {
        let rendered = render_show(&view());

        assert!(rendered.contains("rust-backend-dev (overlay)"));
        assert!(rendered.contains("[inherited_base] 4 chars"));
        assert!(rendered.contains("[disk_overlay] <- role-presets/rust-backend-dev.toml 4 chars"));
        assert!(rendered.contains("[task_overlay] 13 chars"));
        assert!(rendered.contains("=== launch_assignment (preset) ===\nBASE\n\nDISK"));
        assert!(rendered.contains("=== worker_launch_identity (preset) ===\nYou are Mira."));
    }

    #[test]
    fn show_survives_a_view_with_no_fragments() {
        let rendered = render_show(&json!({"kind_id": "worker", "occasions": []}));
        assert!(rendered.contains("(none)"));
    }

    #[test]
    fn list_renders_capability_columns_and_refusals() {
        let rendered = render_list(&json!({
            "overlay_dir": "/home/a/.codeterm/role-presets",
            "kinds": [
                {"kind_id": "orchestrator", "label": "Orchestrator", "origin": "builtin",
                 "anchors_team": true, "team_authority": true, "can_spawn": true},
                {"kind_id": "rust-backend-dev", "label": "Rust Backend Dev", "origin": "overlay",
                 "anchors_team": false, "team_authority": false, "can_spawn": false}
            ],
            "rejected": [
                {"path": "role-presets/hostile.toml", "reason": "capability_escalation",
                 "detail": "declares `team_authority`"}
            ]
        }));

        assert!(rendered.contains("/home/a/.codeterm/role-presets"));
        assert!(rendered.contains("anchors=true authority=true spawn=true"));
        assert!(rendered.contains("anchors=false authority=false spawn=false"));
        assert!(rendered.contains("role-presets/hostile.toml [capability_escalation]"));
    }
}
