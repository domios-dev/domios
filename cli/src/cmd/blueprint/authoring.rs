use super::{pipeline_spec, recipe_files};
use crate::args::blueprint::BlueprintAction;
use codeterm_blueprint_types::blueprint_document::{self, Document};
use serde_json::json;

pub(super) fn source(name: &str) -> Result<String, String> {
    if let Some((_, source)) = recipe_files::FILES.iter().find(|(key, _)| *key == name) {
        return Ok((*source).into());
    }
    pipeline_spec::read_spec(std::path::Path::new(name))
}
pub(super) fn local_bundle(name: &str) -> Result<blueprint_document::calls::LocalBundle, String> {
    let raw = source(name)?;
    blueprint_document::calls::collect_local(name, &raw)
        .map_err(|errors| blueprint_document::checker::render(&errors, true))
}
#[cfg(test)]
pub(super) fn load_checked(name: &str, set: &[String]) -> Result<serde_json::Value, String> {
    let source = source(name)?;
    let checked = blueprint_document::check(name, &source, set);
    if checked.ok {
        checked
            .definition
            .ok_or("Missing checked definition".into())
    } else {
        Err(checked
            .diagnostics
            .iter()
            .filter(|d| d.severity == "error")
            .map(|d| {
                format!(
                    "{}:{}:{}: {}: {} (fix: {})",
                    d.file, d.line, d.column, d.field, d.message, d.hint
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

pub fn handle_local(json_mode: bool, action: &BlueprintAction) -> bool {
    match action {
        BlueprintAction::Schema => {
            crate::cmd::print::print_json(&blueprint_document::schema::schema())
        }
        BlueprintAction::New { kind, like: None } => {
            let raw = scaffold(kind).unwrap_or_else(|error| crate::cmd::print::fail(2, error));
            if json_mode {
                crate::cmd::print::print_json(&json!({"yaml":raw}));
            } else {
                print!("{raw}");
            }
        }
        BlueprintAction::Check {
            source: name,
            set,
            explain,
            fix,
            write,
        } => {
            let raw = source(name).unwrap_or_else(|error| crate::cmd::print::fail(2, error));
            let parsed = blueprint_document::parse(name, &raw);
            let calls = parsed.as_ref().is_ok_and(|parsed| {
                parsed.document.nodes().any(|node| node.call.is_some())
                    || !parsed.document.imports.is_empty()
            });
            if *fix && calls {
                crate::cmd::print::fail(2, "--fix on call documents requires host resolution and is not supported; check without --fix");
            }
            if calls
                || parsed.as_ref().is_ok_and(|parsed| {
                    parsed.document.trigger.is_none() && parsed.document.result.port.is_some()
                })
            {
                return false;
            }
            let context = blueprint_document::checker::CheckContext::default();
            let repair = if *fix {
                blueprint_document::parse(name, &raw).ok().map(|parsed| {
                    blueprint_document::checker::fix::fix_with_assignments(
                        &parsed, &raw, &context, set,
                    )
                })
            } else {
                None
            };
            let effective = repair
                .as_ref()
                .and_then(|r| r.text.as_deref())
                .unwrap_or(&raw);
            let mut checked =
                blueprint_document::check_with_context(name, effective, set, &context);
            if repair.as_ref().is_some_and(|r| !r.refusals.is_empty()) {
                checked.ok = false;
                checked.definition = None;
            }
            if *write && checked.ok {
                if let Some(text) = repair.as_ref().and_then(|r| r.text.as_ref()) {
                    if name == "-" || recipe_files::FILES.iter().any(|(key, _)| key == name) {
                        crate::cmd::print::fail(2, "Write a file path to use --fix --write");
                    }
                    if repair.as_ref().is_some_and(|r| r.changed) {
                        std::fs::write(name, text)
                            .unwrap_or_else(|error| crate::cmd::print::fail(2, error.to_string()));
                    }
                }
            }
            if json_mode {
                let mut value = serde_json::to_value(&checked)
                    .unwrap_or_else(|error| crate::cmd::print::fail(2, error.to_string()));
                if let Some(repair) = &repair {
                    value["fix"] = serde_json::to_value(repair)
                        .unwrap_or_else(|error| crate::cmd::print::fail(2, error.to_string()));
                }
                crate::cmd::print::print_json(&value);
            } else {
                if let Some(repair) = &repair {
                    for note in &repair.notes {
                        println!("fix: {note}");
                    }
                    for refusal in &repair.refusals {
                        println!("fix refused: {refusal}");
                    }
                    print!("{}", repair.diff);
                    if let Some(diff) = &repair.semantic_diff {
                        println!(
                            "semantic diff: {}",
                            serde_json::to_string(diff).unwrap_or_default()
                        );
                    }
                }
                print!(
                    "{}",
                    blueprint_document::checker::render(&checked.diagnostics, *explain)
                );
                if checked.ok {
                    println!("Valid Blueprint: {name}");
                }
            }
            if !checked.ok {
                std::process::exit(2)
            }
        }
        _ => return false,
    }
    true
}

pub(super) fn scaffold(kind: &str) -> Result<String, String> {
    let header="# yaml-language-server: $schema=./blueprint.schema.json\n# Save `domios blueprint schema` beside this file as blueprint.schema.json.\n# Check: domios blueprint check FILE; apply: domios blueprint apply FILE.\n";
    if let Some((_, source)) = recipe_files::FILES.iter().find(|(name, _)| *name == kind) {
        return Ok(format!("{header}{source}"));
    }
    let definition=match kind {
        "watch"=>json!({"schema_version":2,"kind":"watch","name":"custom-watch","note":"Explain the purpose","steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"echo ready"}},"every":{"lit":"20s"},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"State?"}},"buckets":["ready"],"matches":{"ready":{"lit":"ready"}},"routes":{"ready":"alert"}},{"id":"alert","uses":"notify"}]}),
        "pipeline"|"command"=>json!({"schema_version":2,"kind":"pipeline","name":"custom-pipeline","note":"Explain the purpose","owner":"team","trigger":{"type":"schedule","every":{"lit":"1m"}},"steps":[{"id":"inspect","run":{"lit":"echo ready"},"routes":{"done":"@stop","failed":"@stop"}}],"result":{"to":"notify"}}),
        _=>return Err(format!("Unknown recipe or kind {kind}; choose watch, pipeline, command, or a name from blueprint recipe")),
    };
    Ok(format!(
        "{header}{}",
        Document::from_definition(&definition)?.yaml()?
    ))
}

pub(super) fn fail_apply_error(
    name: &str,
    failure: &crate::client::SocketCallError,
    json_mode: bool,
) -> ! {
    use codeterm_blueprint_types::blueprint_contract::ApplyError;
    let typed = match failure {
        crate::client::SocketCallError::Rpc(error) => error
            .data
            .as_ref()
            .and_then(|data| serde_json::from_value::<ApplyError>(data.clone()).ok()),
        _ => None,
    };
    let Some(ApplyError::CreationOnly { field, message }) = typed else {
        crate::cmd::print::fail(1, failure.to_string())
    };
    let raw = if name == "-" {
        String::new()
    } else {
        source(name).unwrap_or_else(|error| {
            crate::cmd::print::fail(
                2,
                format!("{failure}; cannot read source for positioned diagnostic: {error}"),
            )
        })
    };
    let diagnostic = blueprint_document::diagnostic(
        name,
        &raw,
        &field,
        &message,
        "Delete the existing Blueprint, then apply this document again",
    );
    if json_mode {
        crate::cmd::print::print_json(&json!({"ok":false,"diagnostics":[diagnostic]}));
    } else {
        eprintln!(
            "{}:{}:{}: {}: {}\n  fix: {}",
            diagnostic.file,
            diagnostic.line,
            diagnostic.column,
            diagnostic.field,
            diagnostic.message,
            diagnostic.hint
        );
    }
    std::process::exit(2)
}
