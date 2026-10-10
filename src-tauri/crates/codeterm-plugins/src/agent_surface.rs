//! The agent-facing part of `plugin get`: setup help, the verbs reachable
//! through `codeterm plugin <id> <verb>`, and the bundled agent guide.

use std::path::Path;

use codeterm_util::plugin_agent_guide::load_agent_guide;
use serde_json::{json, Map, Value};

pub(super) fn insert_agent_fields(out: &mut Map<String, Value>, dir: &Path, manifest: &Value) {
    out.insert(
        "config_help".to_string(),
        manifest
            .get("configHelp")
            .and_then(Value::as_str)
            .map(|help| Value::String(help.to_string()))
            .unwrap_or(Value::Null),
    );
    out.insert("agent_verbs".to_string(), agent_verbs(manifest));
    out.insert("agent_guide".to_string(), agent_guide(dir, manifest));
}

fn agent_verbs(manifest: &Value) -> Value {
    let verbs = manifest
        .get("agentVerbs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|verb| !verb.is_empty())
        .map(|verb| Value::String(verb.to_string()))
        .collect();
    Value::Array(verbs)
}

fn agent_guide(dir: &Path, manifest: &Value) -> Value {
    let Some(path) = manifest.get("agentGuide").and_then(Value::as_str) else {
        return Value::Null;
    };
    match load_agent_guide(dir, path) {
        Ok(guide) => json!({ "path": path, "text": guide.text, "truncated": guide.truncated }),
        Err(error) => json!({ "path": path, "error": error.to_string() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUIDE_TOKEN: &str = "guide-token-q7";
    const VERB: &str = "zz-alpha <target-q7>";

    fn fields(dir: &Path, manifest: Value) -> Map<String, Value> {
        let mut out = Map::new();
        insert_agent_fields(&mut out, dir, &manifest);
        out
    }

    #[test]
    fn declared_help_verbs_and_guide_are_returned() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("AGENT.md"), GUIDE_TOKEN).unwrap();
        let out = fields(
            dir.path(),
            json!({
                "configHelp": "help-token-q7",
                "agentVerbs": [VERB, "  ", 7],
                "agentGuide": "AGENT.md"
            }),
        );
        assert_eq!(out["config_help"], "help-token-q7");
        assert_eq!(out["agent_verbs"], json!([VERB]));
        assert_eq!(out["agent_guide"]["path"], "AGENT.md");
        assert_eq!(out["agent_guide"]["text"], GUIDE_TOKEN);
        assert_eq!(out["agent_guide"]["truncated"], false);
    }

    #[test]
    fn undeclared_fields_are_null_or_empty() {
        let dir = tempfile::tempdir().unwrap();
        let out = fields(dir.path(), json!({}));
        assert_eq!(out["config_help"], Value::Null);
        assert_eq!(out["agent_verbs"], json!([]));
        assert_eq!(out["agent_guide"], Value::Null);
    }

    #[test]
    fn an_unloadable_guide_reports_its_path_and_error_without_text() {
        let dir = tempfile::tempdir().unwrap();
        for path in ["missing.md", "../AGENT.md"] {
            let out = fields(dir.path(), json!({ "agentGuide": path }));
            assert_eq!(out["agent_guide"]["path"], path);
            assert!(out["agent_guide"]["error"].is_string());
            assert!(out["agent_guide"].get("text").is_none());
        }
    }
}
