//! Agent-facing plugin configuration (plan §1.7).
//!
//! A plugin's settings live in a self-documenting `config.yaml` in its install
//! dir, whose inline comments are the agent docs for each field. `plugin get`
//! returns that YAML (secret values redacted) plus install/configure state;
//! `plugin config` edits it comment-preservingly, validating against the
//! plugin's `settings.schema.json`. Secret-typed fields (schema `api_key`) are
//! NEVER written into the YAML — they are reported for routing to the secret
//! store.

use super::*;
use serde_json::Value;

/// `codeterm plugin get <id>` — the agent's discover handle.
pub(super) fn get_command(id: &str) -> Result<Value, String> {
    let dir = plugin_dir(id);
    if !dir.exists() {
        return Err(format!("plugin '{id}' is not installed"));
    }
    let manifest = read_plugin_manifest_value(&dir)?;
    let (schema, secret_keys) = load_schema(&dir, &manifest);

    let yaml_path = dir.join("config.yaml");
    let raw_yaml = fs::read_to_string(&yaml_path).unwrap_or_default();
    let redacted = redact_yaml(&raw_yaml, &secret_keys);
    let configured = yaml_has_content(&raw_yaml);

    Ok(serde_json::json!({
        "id": id,
        "display_name": manifest.get("displayName").and_then(|v| v.as_str()),
        "description": manifest.get("description").and_then(|v| v.as_str()),
        "installed": true,
        "configured": configured,
        "requires_reload": manifest.get("requiresReload").and_then(|v| v.as_bool()).unwrap_or(false),
        "config_yaml": redacted,
        "settings_schema": schema,
        "secret_fields": secret_keys,
    }))
}

/// Permissions a user/agent may grant or revoke per plugin (§1.10). Mirrors the
/// keys the host reads in `grants::apply_persisted`. Powerful host grants only —
/// allowlist gates writes so a typo can't persist a bogus grant.
const GRANTABLE_PERMS: &[&str] = &[
    "manage_workspaces",
    "spawn_agents",
    "agent_commands",
    "secrets",
];

/// `codeterm plugin config <id> [--set k=v]... [--values-json '{…}'] [--grant
/// P]... [--revoke P]...`.
pub(super) fn config_command(
    id: &str,
    sets: &[String],
    values_json: Option<&str>,
    grants: &[String],
    revokes: &[String],
) -> Result<Value, String> {
    let dir = plugin_dir(id);
    if !dir.exists() {
        return Err(format!("plugin '{id}' is not installed"));
    }
    let manifest = read_plugin_manifest_value(&dir)?;
    let (_schema, secret_keys) = load_schema(&dir, &manifest);
    let known_keys = known_keys(&dir, &manifest);

    let mut updates: Vec<(String, String)> = Vec::new();
    for s in sets {
        let (k, v) = s
            .split_once('=')
            .ok_or_else(|| format!("--set expects KEY=VALUE, got '{s}'"))?;
        updates.push((k.trim().to_string(), v.to_string()));
    }
    if let Some(j) = values_json {
        let obj: serde_json::Map<String, Value> = serde_json::from_str(j)
            .map_err(|e| format!("--values-json must be a JSON object: {e}"))?;
        for (k, v) in obj {
            updates.push((k, json_scalar_to_string(&v)));
        }
    }
    if updates.is_empty() && grants.is_empty() && revokes.is_empty() {
        return Err(
            "nothing to do (use --set KEY=VALUE, --values-json, --grant P, or --revoke P)"
                .to_string(),
        );
    }

    // Validate keys against the schema when one exists (empty → accept any).
    if !known_keys.is_empty() {
        for (k, _) in &updates {
            if !known_keys.contains(k) {
                return Err(format!(
                    "unknown setting '{k}' — not in {id}'s settings schema"
                ));
            }
        }
    }

    // Persist grant/revoke decisions to the plugin's grants.yaml (§1.10). The
    // host reads this fail-closed via grants::apply_persisted on next reload.
    let granted = apply_grants(&dir, grants, revokes)?;

    let yaml_path = dir.join("config.yaml");
    let mut text = fs::read_to_string(&yaml_path).unwrap_or_default();
    let mut changed: Vec<String> = Vec::new();
    let mut secrets_pending: Vec<String> = Vec::new();
    for (k, v) in &updates {
        if secret_keys.contains(k) {
            // Security invariant: a secret never lands in the YAML. Report it for
            // routing to the secret store instead.
            secrets_pending.push(k.clone());
            continue;
        }
        text = yaml_set_scalar(&text, k, v);
        changed.push(k.clone());
    }
    if !updates.is_empty() {
        fs::write(&yaml_path, &text).map_err(|e| format!("write {}: {e}", yaml_path.display()))?;
    }

    Ok(serde_json::json!({
        "ok": true,
        "id": id,
        "changed": changed,
        "secrets_pending": secrets_pending,
        "grants_changed": granted,
        // config.yaml re-stamps live; grants apply on the plugin's next reload.
        "reload": if granted.is_empty() { "live" } else { "reload" },
        "config_path": yaml_path.display().to_string(),
    }))
}

/// Merge grant/revoke decisions into the plugin's `grants.yaml` — a flat
/// `{ permission: bool }` map the host reads via `grants::apply_persisted`. Kept
/// in lockstep with that reader's shape (serde_yaml map). Returns the changed
/// permission names. Validates against [`GRANTABLE_PERMS`] so a typo can't
/// persist a bogus grant.
fn apply_grants(dir: &Path, grants: &[String], revokes: &[String]) -> Result<Vec<String>, String> {
    if grants.is_empty() && revokes.is_empty() {
        return Ok(Vec::new());
    }
    for p in grants.iter().chain(revokes.iter()) {
        if !GRANTABLE_PERMS.contains(&p.as_str()) {
            return Err(format!(
                "'{p}' is not a grantable permission (one of: {})",
                GRANTABLE_PERMS.join(", ")
            ));
        }
    }
    let path = dir.join("grants.yaml");
    let mut map: std::collections::BTreeMap<String, bool> = fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_yaml::from_str(&t).ok())
        .unwrap_or_default();
    let mut changed = Vec::new();
    for p in grants {
        map.insert(p.clone(), true);
        changed.push(p.clone());
    }
    for p in revokes {
        map.insert(p.clone(), false);
        changed.push(p.clone());
    }
    let body = serde_yaml::to_string(&map).map_err(|e| format!("encode grants: {e}"))?;
    fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(changed)
}

fn read_plugin_manifest_value(dir: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(dir.join("plugin.json"))
        .map_err(|e| format!("read plugin.json: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("parse plugin.json: {e}"))
}

/// Load the plugin's `settings.schema.json` (relative to its dir) as a JSON
/// value plus the set of secret-typed field keys (schema `api_key` env_vars).
fn load_schema(dir: &Path, manifest: &Value) -> (Value, Vec<String>) {
    let Some(rel) = manifest.get("settingsSchema").and_then(|v| v.as_str()) else {
        return (Value::Null, Vec::new());
    };
    let Ok(text) = fs::read_to_string(dir.join(rel)) else {
        return (Value::Null, Vec::new());
    };
    let Ok(schema) = serde_json::from_str::<Value>(&text) else {
        return (Value::Null, Vec::new());
    };
    let mut secret = Vec::new();
    collect_secret_keys(&schema, &mut secret);
    (schema, secret)
}

/// All settable field keys in the schema (non-secret + secret). Empty when the
/// plugin ships no schema → `config` accepts any key.
fn known_keys(dir: &Path, manifest: &Value) -> Vec<String> {
    let Some(rel) = manifest.get("settingsSchema").and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(dir.join(rel)) else {
        return Vec::new();
    };
    let Ok(schema) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    collect_all_keys(&schema, &mut keys);
    keys
}

fn collect_secret_keys(node: &Value, out: &mut Vec<String>) {
    walk_fields(node, &mut |field| {
        if field.get("kind").and_then(|v| v.as_str()) == Some("api_key") {
            if let Some(k) = field.get("env_var").and_then(|v| v.as_str()) {
                out.push(k.to_string());
            }
        }
    });
}

fn collect_all_keys(node: &Value, out: &mut Vec<String>) {
    walk_fields(node, &mut |field| {
        let kind = field.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if kind == "api_key" {
            if let Some(k) = field.get("env_var").and_then(|v| v.as_str()) {
                out.push(k.to_string());
            }
        } else if let Some(k) = field.get("key").and_then(|v| v.as_str()) {
            out.push(k.to_string());
        }
    });
}

/// Visit every leaf field object in a `SettingField[]` tree, recursing into
/// `section.fields` and `when.then` groups.
fn walk_fields(node: &Value, f: &mut impl FnMut(&Value)) {
    match node {
        Value::Array(items) => {
            for item in items {
                walk_fields(item, f);
            }
        }
        Value::Object(obj) => {
            if let Some(fields) = obj.get("fields") {
                walk_fields(fields, f);
            }
            if let Some(then) = obj.get("then") {
                walk_fields(then, f);
            }
            f(node);
        }
        _ => {}
    }
}

/// True when the YAML has any non-comment, non-blank content.
fn yaml_has_content(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .any(|l| !l.is_empty() && !l.starts_with('#'))
}

/// Set a top-level `key: value`, preserving comments. Replaces the value of an
/// existing `key:` line (keeping any trailing ` # comment`), otherwise appends a
/// new `key: value` line. The value is YAML-encoded so strings needing quotes
/// are quoted correctly.
fn yaml_set_scalar(text: &str, key: &str, value: &str) -> String {
    let encoded = encode_scalar(value);
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let mut replaced = false;
    for line in lines.iter_mut() {
        if let Some(rest) = top_level_key(line, key) {
            let comment = rest
                .find(" #")
                .map(|i| rest[i..].to_string())
                .unwrap_or_default();
            *line = format!("{key}: {encoded}{comment}");
            replaced = true;
            break;
        }
    }
    if !replaced {
        if !text.is_empty() && !text.ends_with('\n') {
            lines.push(String::new());
        }
        lines.push(format!("{key}: {encoded}"));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Redact every secret field's value to `"***"` in place, preserving comments
/// and all other lines. Only rewrites keys already present.
fn redact_yaml(text: &str, secret_keys: &[String]) -> String {
    if secret_keys.is_empty() || text.is_empty() {
        return text.to_string();
    }
    text.lines()
        .map(|line| {
            for key in secret_keys {
                if let Some(rest) = top_level_key(line, key) {
                    let comment = rest
                        .find(" #")
                        .map(|i| rest[i..].to_string())
                        .unwrap_or_default();
                    return format!("{key}: \"***\"{comment}");
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// If `line` is a top-level `key: …`, return the part after the colon.
fn top_level_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    // Top-level only: no leading indentation.
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = line.strip_prefix(key)?;
    let rest = rest.strip_prefix(':')?;
    Some(rest)
}

fn encode_scalar(value: &str) -> String {
    serde_yaml::to_string(&Value::String(value.to_string()))
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| format!("\"{value}\""))
}

fn json_scalar_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_scalar_replaces_value_preserving_comment() {
        let yaml = "# the speech engine\nengine: whisper # default\nport: 8080\n";
        let out = yaml_set_scalar(yaml, "engine", "deepgram");
        assert!(out.contains("# the speech engine"), "leading comment kept");
        assert!(
            out.contains("engine: deepgram # default"),
            "inline comment kept: {out}"
        );
        assert!(out.contains("port: 8080"), "other lines kept");
    }

    #[test]
    fn set_scalar_appends_unknown_key() {
        let out = yaml_set_scalar("engine: whisper\n", "serverUrl", "http://x");
        assert!(out.contains("engine: whisper"));
        assert!(out.contains("serverUrl: http://x"));
    }

    #[test]
    fn redact_only_secret_fields_keeps_comments() {
        let yaml = "engine: whisper\napiKey: sk-secret # paste here\n";
        let out = redact_yaml(yaml, &["apiKey".to_string()]);
        assert!(out.contains("engine: whisper"));
        assert!(!out.contains("sk-secret"), "secret value must be gone");
        assert!(
            out.contains("apiKey: \"***\" # paste here"),
            "redacted + comment: {out}"
        );
    }

    #[test]
    fn yaml_has_content_ignores_comments_and_blanks() {
        assert!(!yaml_has_content("# just a comment\n\n"));
        assert!(yaml_has_content("engine: whisper\n"));
    }

    fn temp_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ct-grants-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn apply_grants_writes_and_merges_grants_yaml() {
        let dir = temp_dir();
        // Grant one, revoke another → grants.yaml the host reads via apply_persisted.
        let changed = apply_grants(
            &dir,
            &["spawn_agents".to_string()],
            &["manage_workspaces".to_string()],
        )
        .unwrap();
        assert!(changed.contains(&"spawn_agents".to_string()));
        let text = std::fs::read_to_string(dir.join("grants.yaml")).unwrap();
        let map: std::collections::BTreeMap<String, bool> = serde_yaml::from_str(&text).unwrap();
        assert_eq!(map.get("spawn_agents"), Some(&true));
        assert_eq!(map.get("manage_workspaces"), Some(&false));

        // A later grant merges (doesn't clobber the prior decision).
        apply_grants(&dir, &["agent_commands".to_string()], &[]).unwrap();
        let map2: std::collections::BTreeMap<String, bool> =
            serde_yaml::from_str(&std::fs::read_to_string(dir.join("grants.yaml")).unwrap())
                .unwrap();
        assert_eq!(map2.get("spawn_agents"), Some(&true));
        assert_eq!(map2.get("agent_commands"), Some(&true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_grants_rejects_unknown_permission() {
        let dir = temp_dir();
        let err = apply_grants(&dir, &["take_over_the_world".to_string()], &[]).unwrap_err();
        assert!(err.contains("not a grantable permission"), "{err}");
        assert!(
            !dir.join("grants.yaml").exists(),
            "a rejected grant must not write a file"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_secret_keys_finds_api_key_in_nested_section() {
        let schema = serde_json::json!([
            {"kind":"section","title":"Auth","fields":[
                {"kind":"api_key","env_var":"OPENAI_API_KEY","label":"Key"},
                {"kind":"text","key":"serverUrl","label":"URL"}
            ]}
        ]);
        let mut secret = Vec::new();
        collect_secret_keys(&schema, &mut secret);
        assert_eq!(secret, vec!["OPENAI_API_KEY".to_string()]);
        let mut all = Vec::new();
        collect_all_keys(&schema, &mut all);
        assert!(all.contains(&"serverUrl".to_string()));
        assert!(all.contains(&"OPENAI_API_KEY".to_string()));
    }
}
