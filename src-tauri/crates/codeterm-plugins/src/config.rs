//! Agent-facing plugin configuration. Settings live in a self-documenting
//! `config.yaml`; `plugin get` returns it (secrets redacted), `plugin config`
//! edits it comment-preservingly against `settings.schema.json`. Secret-typed
//! fields are never written into the YAML — they route to the secret store.

use super::*;
use codeterm_util::persisted_state::{write_atomic, Loaded, StateFile};
use serde_json::Value;
use std::collections::BTreeMap;

/// `codeterm plugin get <id>` — the agent's discover handle.
pub(super) fn get_command(id: &str) -> Result<Value, String> {
    let id = PluginId::parse(id.to_string())?;
    let dir = plugin_dir(&id);
    if !dir.exists() {
        if is_reserved_system_id(id.as_str()) {
            return Err(format!(
                "plugin '{}' is bundled with this build and not extracted to the plugins directory, so it has no config to show",
                id.as_str()
            ));
        }
        return Err(format!("plugin '{}' is not installed", id.as_str()));
    }
    let manifest = read_plugin_manifest_value(&dir)?;
    let (schema, secret_keys) = load_schema(&dir, &manifest);

    let yaml_path = dir.join("config.yaml");
    let raw_yaml = fs::read_to_string(&yaml_path).unwrap_or_default();
    let redacted = redact_yaml(&raw_yaml, &secret_keys);
    let configured = yaml_has_content(&raw_yaml);

    Ok(serde_json::json!({
        "id": id.as_str(),
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

/// Grantable per-plugin permissions; an allowlist so a typo can't persist a bogus grant.
const GRANTABLE_PERMS: &[&str] = &[
    "manage_workspaces",
    "spawn_agents",
    "agent_commands",
    "secrets",
];

/// `codeterm plugin config <id> [--set k=v]... [--values-json '{…}'] [--grant P]... [--revoke P]...`.
pub(super) fn config_command(
    id: &str,
    sets: &[String],
    values_json: Option<&str>,
    grants: &[String],
    revokes: &[String],
) -> Result<Value, String> {
    let parsed_id = PluginId::parse(id.to_string())?;
    let dir = plugin_dir(&parsed_id);
    if !dir.exists() {
        return Err(format!("plugin '{id}' is not installed"));
    }
    let manifest = read_plugin_manifest_value(&dir)?;
    let (_schema, secret_keys) = load_schema(&dir, &manifest);
    let known_keys = known_keys(&dir, &manifest);

    let mut updates: Vec<(String, Value)> = Vec::new();
    for s in sets {
        let (k, v) = s
            .split_once('=')
            .ok_or_else(|| format!("--set expects KEY=VALUE, got '{s}'"))?;
        // Infer the scalar type so `enabled=false` writes a real YAML boolean, not the truthy string "false".
        updates.push((k.trim().to_string(), infer_scalar(v)));
    }
    if let Some(j) = values_json {
        let obj: serde_json::Map<String, Value> = serde_json::from_str(j)
            .map_err(|e| format!("--values-json must be a JSON object: {e}"))?;
        for (k, v) in obj {
            // The JSON value already carries its type — preserve it verbatim.
            updates.push((k, v));
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

    // Persist grant/revoke to grants.yaml; the host reads it fail-closed on next reload.
    let granted = apply_grants(&dir, grants, revokes)?;

    let yaml_path = dir.join("config.yaml");
    let mut text = match fs::read_to_string(&yaml_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("read {}: {e}", yaml_path.display())),
    };
    let mut changed: Vec<String> = Vec::new();
    let mut secrets_pending: Vec<String> = Vec::new();
    for (k, v) in &updates {
        if secret_keys.contains(k) {
            // Security invariant: a secret never lands in the YAML — report it for the secret store instead.
            secrets_pending.push(k.clone());
            continue;
        }
        text = yaml_set_scalar(&text, k, v);
        changed.push(k.clone());
    }
    if !updates.is_empty() {
        write_atomic(&yaml_path, text.as_bytes())
            .map_err(|e| format!("write {}: {e}", yaml_path.display()))?;
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

/// Merge grant/revoke into the plugin's `grants.yaml` (a flat `{ permission: bool }`
/// map the host reads), validated against [`GRANTABLE_PERMS`]. Returns the changed names.
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
    let file = grants_file(dir);
    let mut map = match file.load(parse_grants) {
        Loaded::Parsed(map) | Loaded::Recovered { value: map, .. } => map,
        Loaded::Missing if file.held().is_none() => BTreeMap::new(),
        Loaded::Missing | Loaded::Unrecoverable { .. } => {
            return Err(file
                .held()
                .map(|hold| hold.refusal())
                .unwrap_or_else(|| format!("{} failed to load", file.path().display())))
        }
    };
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
    file.save(&body)?;
    Ok(changed)
}

/// Same file and rules as the host reader: no rotating backups inside the bundle.
fn grants_file(dir: &Path) -> StateFile {
    StateFile::new(dir.join("grants.yaml"))
        .without_backups()
        .with_validator(validate_grants)
}

fn parse_grants(text: &str) -> Result<BTreeMap<String, bool>, String> {
    if text.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    serde_yaml::from_str(text).map_err(|e| e.to_string())
}

fn validate_grants(text: &str) -> Result<(), String> {
    parse_grants(text).map(|_| ())
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

/// Infer the scalar type of a `--set KEY=VALUE` string so booleans/numbers land in
/// the YAML natively — else `enabled=false` stores as the truthy string "false" and the toggle never takes effect.
fn infer_scalar(s: &str) -> Value {
    match s {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(i) = s.parse::<i64>() {
        return Value::from(i);
    }
    // Only a float when it round-trips ("1.2.3"/"http://x" stay strings); Number guards NaN/inf.
    if let Ok(f) = s.parse::<f64>() {
        if let Some(n) = serde_json::Number::from_f64(f) {
            return Value::Number(n);
        }
    }
    Value::String(s.to_string())
}

/// Set a top-level `key: value`, preserving comments: replace an existing line
/// (keeping its trailing comment) or append. Value is YAML-encoded by type.
fn yaml_set_scalar(text: &str, key: &str, value: &Value) -> String {
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

/// YAML-encode a typed scalar to a single line. Booleans/numbers emit natively
/// (`false`, `8080`); strings are quoted only when YAML requires it.
fn encode_scalar(value: &Value) -> String {
    serde_yaml::to_string(value)
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_scalar_replaces_value_preserving_comment() {
        let yaml = "# the speech engine\nengine: whisper # default\nport: 8080\n";
        let out = yaml_set_scalar(yaml, "engine", &infer_scalar("deepgram"));
        assert!(out.contains("# the speech engine"), "leading comment kept");
        assert!(
            out.contains("engine: deepgram # default"),
            "inline comment kept: {out}"
        );
        assert!(out.contains("port: 8080"), "other lines kept");
    }

    #[test]
    fn set_scalar_appends_unknown_key() {
        let out = yaml_set_scalar("engine: whisper\n", "serverUrl", &infer_scalar("http://x"));
        assert!(out.contains("engine: whisper"));
        assert!(out.contains("serverUrl: http://x"));
    }

    #[test]
    fn set_bool_writes_native_yaml_bool_not_string() {
        // `--set enabled=false` must write a real boolean, not the truthy string "false".
        let out = yaml_set_scalar("enabled: 'true'\n", "enabled", &infer_scalar("false"));
        assert!(
            out.contains("enabled: false"),
            "native bool expected: {out}"
        );
        assert!(
            !out.contains("'false'") && !out.contains("\"false\""),
            "must not be a quoted string: {out}"
        );
    }

    #[test]
    fn infer_scalar_types_bool_number_string() {
        assert_eq!(infer_scalar("true"), serde_json::json!(true));
        assert_eq!(infer_scalar("false"), serde_json::json!(false));
        assert_eq!(infer_scalar("8080"), serde_json::json!(8080));
        assert_eq!(infer_scalar("0.5"), serde_json::json!(0.5));
        assert_eq!(infer_scalar("whisper"), serde_json::json!("whisper"));
        // A version-like / url string stays a string (not a number).
        assert_eq!(infer_scalar("http://x"), serde_json::json!("http://x"));
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
    fn apply_grants_refuses_to_replace_an_unparseable_grants_file() {
        let dir = temp_dir();
        let broken = "manage_workspaces: false
spawn_agents: [
";
        std::fs::write(dir.join("grants.yaml"), broken).unwrap();

        assert!(apply_grants(&dir, &["agent_commands".to_string()], &[]).is_err());

        assert!(!dir.join("grants.yaml").exists());
        let copies = grants_file(&dir).corrupt_copies();
        assert_eq!(std::fs::read_to_string(&copies[0]).unwrap(), broken);
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
