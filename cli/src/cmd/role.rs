use serde_json::{json, Value};

use crate::args::role::RoleAction;
use crate::client::SocketClient;

pub fn handle(
    client: &mut SocketClient,
    json_output: bool,
    action: RoleAction,
) -> Result<Option<Value>, String> {
    match action {
        RoleAction::List { team } => registry(client, json_output, "role.list", team),
        RoleAction::Reload { team } => registry(client, json_output, "role.reload", team),
        RoleAction::Show {
            role_type,
            team,
            guidance,
            occasion,
        } => {
            let mut params = json!({ "type": role_type });
            for (key, value) in [
                ("team_id", team),
                ("guidance", guidance),
                ("occasion", occasion),
            ] {
                if let Some(value) = value {
                    params[key] = json!(value);
                }
            }
            let result = client.call("role.show", Some(params))?;
            if json_output {
                return Ok(Some(result));
            }
            println!("{}", render_show(&result));
            Ok(None)
        }
    }
}

fn registry(
    client: &mut SocketClient,
    json_output: bool,
    method: &str,
    team: Option<String>,
) -> Result<Option<Value>, String> {
    let params = match team {
        Some(team) => json!({ "team_id": team }),
        None => json!({}),
    };
    let result = client.call(method, Some(params))?;
    if json_output {
        return Ok(Some(result));
    }
    println!("{}", render_list(&result));
    Ok(None)
}

fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn provenance(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return "unregistered".to_string();
    };
    let package = value.get("package").unwrap_or(&Value::Null);
    format!(
        "{} {}@{}",
        field(value, "source"),
        field(package, "name"),
        field(package, "version")
    )
}

fn render_show(view: &Value) -> String {
    let mut out = format!(
        "{} ({})\nkind: {}\nauthority: {}\nrole: {}\nlabel: {}\n\nfragments:\n",
        field(view, "type"),
        provenance(view.get("provenance")),
        field(view, "kind"),
        field(view, "authority"),
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
        let package = fragment
            .get("package")
            .map(|package| {
                format!(
                    " <- {}@{}",
                    field(package, "name"),
                    field(package, "version")
                )
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "  [{}]{} {} chars\n",
            field(fragment, "source"),
            package,
            field(fragment, "text").len()
        ));
    }
    for occasion in view
        .get("occasions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
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
    let mut out = match view.get("team_id").and_then(Value::as_str) {
        Some(team) => format!("team: {team}\n\nroles:\n"),
        None => "team: (builtin default package)\n\nroles:\n".to_string(),
    };
    for role in view
        .get("roles")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let host = if role.get("host_only").and_then(Value::as_bool) == Some(true) {
            " host-only"
        } else {
            ""
        };
        out.push_str(&format!(
            "  {:<16} {:<8} {:<22} {} authority={} anchors={} team_authority={} spawn={}{host}\n",
            field(&role, "type"),
            field(&role, "kind"),
            field(&role, "label"),
            provenance(role.get("provenance")),
            field(&role, "authority"),
            role.get("anchors_team").and_then(Value::as_bool) == Some(true),
            role.get("team_authority").and_then(Value::as_bool) == Some(true),
            role.get("can_spawn").and_then(Value::as_bool) == Some(true),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{render_list, render_show};

    #[test]
    fn show_renders_provenance_every_fragment_and_occasion_text_verbatim() {
        let rendered = render_show(&json!({
            "type": "checker",
            "role": "Worker",
            "kind": "worker",
            "authority": "none",
            "label": "Checker",
            "provenance": {"source": "team", "package": {"name": "com.example/desk", "version": "2.1.0"}},
            "fragments": [
                {"source": "team_registration", "package": {"name": "com.example/desk", "version": "2.1.0"}, "text": "BASE"},
                {"source": "host", "text": "HOST"},
                {"source": "task_overlay", "text": "You are Mira."}
            ],
            "occasions": [
                {"occasion": "launch_assignment", "emission": "preset", "text": "BASE\n\nHOST"}
            ]
        }));

        assert!(rendered.contains("checker (team com.example/desk@2.1.0)"));
        assert!(rendered.contains("kind: worker\nauthority: none\n"));
        assert!(rendered.contains("[team_registration] <- com.example/desk@2.1.0 4 chars"));
        assert!(rendered.contains("[host] 4 chars"));
        assert!(rendered.contains("[task_overlay] 13 chars"));
        assert!(rendered.contains("=== launch_assignment (preset) ===\nBASE\n\nHOST"));
    }

    #[test]
    fn show_survives_a_view_with_no_fragments() {
        let rendered = render_show(&json!({"type": "worker", "occasions": []}));
        assert!(rendered.contains("(none)"));
    }

    #[test]
    fn list_renders_provenance_and_capability_columns() {
        let rendered = render_list(&json!({
            "team_id": "team_a",
            "roles": [
                {"type": "orchestrator", "kind": "manager", "authority": "Orchestrator", "label": "Orchestrator",
                 "provenance": {"source": "builtin", "package": {"name": "dev.domios/default", "version": "1.0.0"}},
                 "anchors_team": true, "team_authority": true, "can_spawn": true, "host_only": false},
                {"type": "security_lead", "kind": "manager", "authority": "none", "label": "Warden",
                 "provenance": {"source": "team", "package": {"name": "com.example/desk", "version": "2.1.0"}},
                 "anchors_team": false, "team_authority": false, "can_spawn": false, "host_only": false},
                {"type": "watcher", "kind": "manager", "label": "Watcher",
                 "provenance": {"source": "builtin", "package": {"name": "dev.domios/default", "version": "1.0.0"}},
                 "host_only": true}
            ]
        }));

        assert!(rendered.contains("team: team_a"));
        assert!(rendered.contains(
            "builtin dev.domios/default@1.0.0 authority=Orchestrator anchors=true team_authority=true spawn=true"
        ));
        assert!(rendered.contains("security_lead    manager  Warden"));
        assert!(rendered.contains("authority=none anchors=false team_authority=false spawn=false"));
        assert!(rendered.contains("host-only"));
    }
}
