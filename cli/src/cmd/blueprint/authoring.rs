use super::{pipeline_spec, recipe_files};
use crate::args::blueprint::BlueprintAction;
use codeterm_blueprint_types::blueprint_document::{self, Document};
use serde_json::{json, Value};

pub(super) fn source(name: &str) -> Result<String, String> {
    if let Some((_, source)) = recipe_files::FILES.iter().find(|(key, _)| *key == name) {
        return Ok((*source).into());
    }
    pipeline_spec::read_spec(std::path::Path::new(name))
}
pub(super) fn load_checked(name: &str, set: &[String]) -> Result<Value, String> {
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
        BlueprintAction::New { kind } => {
            let raw = scaffold(kind).unwrap_or_else(|error| crate::cmd::print::fail(2, error));
            if json_mode {
                crate::cmd::print::print_json(&json!({"yaml":raw}));
            } else {
                print!("{raw}");
            }
        }
        BlueprintAction::Check { source: name, set } => {
            let raw = source(name).unwrap_or_else(|error| crate::cmd::print::fail(2, error));
            let checked = blueprint_document::check(name, &raw, set);
            if json_mode {
                crate::cmd::print::print_json(&serde_json::to_value(&checked).unwrap_or_default());
            } else {
                for d in &checked.diagnostics {
                    println!(
                        "{}:{}:{}: {}: {}: {}\n  fix: {}",
                        d.file, d.line, d.column, d.severity, d.field, d.message, d.hint
                    );
                }
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
    let header="# yaml-language-server: $schema=./blueprint.schema.json\n# Save `codeterm blueprint schema` beside this file as blueprint.schema.json.\n# Check: codeterm blueprint check FILE; apply: codeterm blueprint apply FILE.\n";
    if let Some((_, source)) = recipe_files::FILES.iter().find(|(name, _)| *name == kind) {
        return Ok(format!("{header}{source}"));
    }
    let definition=match kind {
        "watch"=>json!({"create":{"name":"custom-watch","note":"Explain the purpose","probe":"echo ready","question":"State?","buckets":["ready"],"every":"20s","matches":{"ready":"ready"},"routes":{"ready":"notify"},"timeout":"1h"}}),
        "pipeline"|"command"=>json!({"spec":{"name":"custom-pipeline","note":"Explain the purpose","owner":"team","trigger":{"type":"schedule","every":"1m"},"steps":[{"name":"inspect","input":"text","kind":{"kind":"command","command":"echo ready"},"on":{"done":"stop","failed":"stop"}}],"result":{"to":"notify"}}}),
        _=>return Err(format!("Unknown recipe or kind {kind}; choose watch, pipeline, command, or a name from blueprint recipe")),
    };
    Ok(format!(
        "{header}{}",
        Document::from_definition(&definition)?.yaml()?
    ))
}

pub(super) fn fail_apply_error(name: &str, error: &str, json_mode: bool) -> ! {
    let Some((_, tail)) = error.split_once("blueprint_creation_only:") else {
        crate::cmd::print::fail(1, error)
    };
    let (field, message) = tail.split_once(':').unwrap_or(("watch", tail));
    let raw = if name == "-" {
        String::new()
    } else {
        source(name).unwrap_or_default()
    };
    let diagnostic = blueprint_document::diagnostic(
        name,
        &raw,
        field,
        message,
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
