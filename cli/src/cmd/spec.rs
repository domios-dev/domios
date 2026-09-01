use crate::args::spec::{SpecAction, SpecLinkAction, SpecSectionAction};
use crate::client::RpcClient;
use crate::cmd::hints::{push_hints, spec_create_next_plan_hint, spec_section_add_link_hint};
use serde_json::{json, Value};

fn call(client: &mut dyn RpcClient, method: &str, params: Value) -> Result<Value, String> {
    crate::cmd::plan::call_plan_rpc(client, method, params)
}

fn render_spec_markdown(spec: &Value) -> String {
    let spec = spec.get("spec").unwrap_or(spec);
    let mut out = String::new();
    let title = spec.get("title").and_then(|v| v.as_str()).unwrap_or("?");
    let id = spec.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    out.push_str(&format!("# {title} (`{id}`)\n\n"));
    if let Some(sections) = spec.get("sections").and_then(|v| v.as_array()) {
        for section in sections {
            render_section(section, 2, &mut out);
        }
    }
    out
}

/// One row per spec, columns instead of a raw JSON dump — the interactive
/// form must never be worse than `--json`.
fn render_spec_list(val: &Value) -> String {
    let rows = match val.get("specs").and_then(Value::as_array) {
        Some(rows) => rows,
        None => return "(no specs)".to_string(),
    };
    if rows.is_empty() {
        return "(no specs)".to_string();
    }
    let mut out = String::new();
    for (i, spec) in rows.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!(
            "{}\t{}\t{}\t{}",
            spec.get("spec_id").and_then(Value::as_str).unwrap_or("?"),
            spec.get("status").and_then(Value::as_str).unwrap_or("?"),
            spec.get("title").and_then(Value::as_str).unwrap_or("?"),
            spec.get("project_path")
                .and_then(Value::as_str)
                .unwrap_or("?"),
        ));
    }
    out
}

fn render_section(section: &Value, depth: usize, out: &mut String) {
    let title = section.get("title").and_then(|v| v.as_str()).unwrap_or("?");
    out.push_str(&format!("{} {title}\n\n", "#".repeat(depth.min(6))));
    if let Some(body) = section.get("body").and_then(|v| v.as_str()) {
        if !body.is_empty() {
            out.push_str(body);
            out.push_str("\n\n");
        }
    }
    if let Some(children) = section.get("subsections").and_then(|v| v.as_array()) {
        for child in children {
            render_section(child, depth + 1, out);
        }
    }
}

pub fn handle_spec(client: &mut dyn RpcClient, json_output: bool, action: SpecAction) {
    match run_spec(client, json_output, action) {
        Ok(None) => {}
        Ok(Some(val)) => {
            crate::cmd::plan_output::print_plan_result(json_output, &val);
        }
        Err(e) => {
            crate::cmd::plan_output::print_plan_error(json_output, &e);
            std::process::exit(1);
        }
    }
}

fn run_spec(
    client: &mut dyn RpcClient,
    json_output: bool,
    action: SpecAction,
) -> Result<Option<Value>, String> {
    let result = match action {
        SpecAction::Create { title, project } => {
            // Default to the caller's cwd — the daemon's own cwd is meaningless
            // (often / when launched via `open -a`, which is read-only).
            let project = project.or_else(|| {
                (!client.is_remote()).then(|| {
                    std::env::current_dir()
                        .ok()
                        .map(|d| d.display().to_string())
                })?
            });
            let mut params = json!({ "title": title });
            if let Some(project) = project {
                params["project_path"] = json!(project);
            }
            let mut val = call(client, "spec.create", params)?;
            let spec_id = val
                .get("spec_id")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if let Some(spec_id) = spec_id.as_deref() {
                push_hints(&mut val, vec![spec_create_next_plan_hint(spec_id)]);
            }
            Ok(val)
        }
        SpecAction::List {
            project,
            active,
            archived,
            all,
        } => {
            let mut params = json!({});
            if let Some(project) = project {
                params["project"] = json!(project);
            }
            if active {
                params["status"] = json!("active");
            } else if archived {
                params["status"] = json!("archived");
            } else if !all {
                params["status"] = json!("active");
            }
            let val = call(client, "spec.list", params)?;
            if json_output {
                return Ok(Some(val));
            }
            println!("{}", render_spec_list(&val));
            return Ok(None);
        }
        SpecAction::Show { spec_id } => {
            let val = call(
                client,
                "spec.show",
                json!({ "spec_id": spec_id, "full": true }),
            )?;
            if json_output {
                return Ok(Some(val));
            }
            println!("{}", render_spec_markdown(&val));
            return Ok(None);
        }
        SpecAction::Delete { spec_id, yes } => {
            if !yes {
                return Err("refusing to delete without --yes".into());
            }
            call(client, "spec.delete", json!({ "spec_id": spec_id }))
        }
        SpecAction::Section { action } => match action {
            SpecSectionAction::Add {
                spec_id,
                title,
                body,
                body_file,
                parent,
                tags,
                primary,
                handle,
            } => {
                let body = crate::cmd::plan::resolve_text(body, body_file)?.unwrap_or_default();
                let mut params = json!({ "spec_id": spec_id, "title": title, "body": body });
                if let Some(parent) = parent {
                    params["parent"] = json!(parent);
                }
                if !tags.is_empty() {
                    params["tags"] = json!(tags);
                }
                if let Some(primary) = primary {
                    params["primary"] = json!(primary);
                }
                if let Some(handle) = handle {
                    params["handle"] = json!(handle);
                }
                let mut val = call(client, "spec.section.add", params)?;
                push_hints(&mut val, vec![spec_section_add_link_hint(&spec_id)]);
                Ok(val)
            }
            SpecSectionAction::Update {
                spec_id,
                section_id,
                title,
                body,
                body_file,
                tags,
                primary,
                handle,
            } => {
                let body = crate::cmd::plan::resolve_text(body, body_file)?;
                let mut params = json!({ "spec_id": spec_id, "section_id": section_id });
                if let Some(title) = title {
                    params["title"] = json!(title);
                }
                if let Some(body) = body {
                    params["body"] = json!(body);
                }
                if !tags.is_empty() {
                    params["tags"] = json!(tags);
                }
                if let Some(primary) = primary {
                    params["primary"] = json!(primary);
                }
                if let Some(handle) = handle {
                    params["handle"] = json!(handle);
                }
                call(client, "spec.section.update", params)
            }
            SpecSectionAction::Show {
                spec_id,
                section_id,
            } => {
                let val = call(
                    client,
                    "spec.section.show",
                    json!({ "spec_id": spec_id, "section_id": section_id }),
                )?;
                if json_output {
                    return Ok(Some(val));
                }
                print!("{}", crate::cmd::plan_sections::render_section_show(&val));
                return Ok(None);
            }
            SpecSectionAction::Rm {
                spec_id,
                section_id,
            } => call(
                client,
                "spec.section.rm",
                json!({ "spec_id": spec_id, "section_id": section_id }),
            ),
        },
        SpecAction::Link { action } => match action {
            SpecLinkAction::Add {
                plan_id,
                spec,
                spec_section,
                plan_section,
                edges,
            } => {
                let mut edge_list: Vec<Value> = Vec::new();
                if let (Some(ss), Some(ps)) = (spec_section, plan_section) {
                    edge_list.push(json!({ "spec_section_id": ss, "plan_section_id": ps }));
                }
                for edge in &edges {
                    let (ss, ps) = edge.split_once(':').ok_or_else(|| {
                        format!("invalid --edge '{edge}': expected SPEC_SECTION:PLAN_SECTION")
                    })?;
                    let (ss, ps) = (ss.trim(), ps.trim());
                    if ss.is_empty() || ps.is_empty() {
                        return Err(format!(
                            "invalid --edge '{edge}': both sides must be non-empty"
                        ));
                    }
                    edge_list.push(json!({ "spec_section_id": ss, "plan_section_id": ps }));
                }
                if edge_list.is_empty() {
                    return Err(
                        "spec link add needs --spec-section/--plan-section or at least one --edge"
                            .into(),
                    );
                }
                call(
                    client,
                    "spec.link.add",
                    json!({ "plan_id": plan_id, "spec_id": spec, "edges": edge_list }),
                )
            }
            SpecLinkAction::Rm {
                plan_id,
                spec,
                spec_section,
                plan_section,
            } => {
                let mut params = json!({ "plan_id": plan_id, "spec_id": spec });
                if let Some(spec_section) = spec_section {
                    params["spec_section_id"] = json!(spec_section);
                }
                if let Some(plan_section) = plan_section {
                    params["plan_section_id"] = json!(plan_section);
                }
                call(client, "spec.link.rm", params)
            }
            SpecLinkAction::List { spec_id } => {
                call(client, "spec.link.list", json!({ "spec_id": spec_id }))
            }
        },
    }?;
    if json_output {
        Ok(Some(result))
    } else {
        crate::cmd::plan_output::print_plan_result(false, &result);
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// codeterm-releases#60: `spec list` renders one row per spec in columns,
    /// never the raw `{"..."` JSON dump `print_object`'s fallback produced.
    #[test]
    fn spec_list_renders_columns_without_json() {
        let val = json!({
            "specs": [
                {
                    "spec_id": "spec_a1",
                    "status": "active",
                    "title": "First Spec",
                    "project_path": "/repo/one",
                    "file_path": "/repo/one/.codeterm/specs/spec_a1.md",
                    "updated_at": 1,
                },
                {
                    "spec_id": "spec_b2",
                    "status": "archived",
                    "title": "Second Spec",
                    "project_path": "/repo/two",
                    "file_path": "/repo/two/.codeterm/specs/spec_b2.md",
                    "updated_at": 2,
                },
            ]
        });

        let rendered = render_spec_list(&val);

        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 2, "one line per record, not one line total");
        for line in &lines {
            assert!(
                !line.contains("{\""),
                "no line may contain a raw JSON object: {line}"
            );
        }
        assert!(lines[0].contains("spec_a1"));
        assert!(lines[0].contains("active"));
        assert!(lines[0].contains("First Spec"));
        assert!(lines[0].contains("/repo/one"));
        assert!(lines[1].contains("spec_b2"));
        assert!(lines[1].contains("archived"));
        assert!(lines[1].contains("Second Spec"));
        assert!(lines[1].contains("/repo/two"));
    }

    #[test]
    fn spec_list_renders_empty_state_without_json() {
        assert_eq!(render_spec_list(&json!({ "specs": [] })), "(no specs)");
    }

    #[derive(Default)]
    struct FakeRpc {
        response: Value,
    }

    impl RpcClient for FakeRpc {
        fn call_detailed(
            &mut self,
            _method: &str,
            _params: Option<Value>,
        ) -> Result<Value, crate::client::SocketCallError> {
            Ok(self.response.clone())
        }
    }

    /// codeterm-releases#60: `--json` still returns the daemon's raw
    /// response untouched, never routed through `render_spec_list`.
    #[test]
    fn spec_list_json_mode_returns_the_raw_daemon_response() {
        let daemon_response = json!({
            "specs": [{ "spec_id": "spec_a1", "status": "active", "title": "First Spec" }]
        });
        let mut client = FakeRpc {
            response: daemon_response.clone(),
        };

        let result = run_spec(
            &mut client,
            true,
            SpecAction::List {
                project: None,
                active: false,
                archived: false,
                all: true,
            },
        )
        .unwrap();

        assert_eq!(result, Some(daemon_response));
    }
}
