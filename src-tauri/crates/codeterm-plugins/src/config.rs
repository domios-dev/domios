//! Agent-facing plugin configuration. Settings live in a self-documenting
//! `config.yaml`; `plugin get` returns it (secrets redacted), `plugin config`
//! edits it comment-preservingly against `settings.schema.json`. Secret-typed
//! fields are never written into the YAML — they route to the secret store.

use super::*;
use codeterm_util::persisted_state::{write_atomic, Loaded, StateFile};
use codeterm_util::secret_file::SecretFile;
use serde_json::Value;

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

    let yaml_path = config_yaml_path(&id, &dir);
    let raw_yaml = fs::read_to_string(&yaml_path).unwrap_or_default();
    let redacted = redact_yaml(&raw_yaml, &secret_keys);
    let bucket = plugin_secret_bucket(id.as_str());
    if !secret_keys.is_empty() && !bucket.is_available() {
        return Err("plugin secret bucket is unavailable".to_string());
    }
    let mut secret_status = serde_json::Map::new();
    for key in &secret_keys {
        let set = bucket
            .has(key)
            .map_err(|error| format!("read secret status for '{key}': {error}"))?;
        secret_status.insert(
            key.clone(),
            Value::String(if set { "set" } else { "unset" }.to_string()),
        );
    }
    let configured = yaml_has_content(&raw_yaml)
        || secret_status
            .values()
            .any(|value| value.as_str() == Some("set"));

    let mut out = serde_json::json!({
        "id": id.as_str(),
        "display_name": manifest.get("displayName").and_then(|v| v.as_str()),
        "description": manifest.get("description").and_then(|v| v.as_str()),
        "installed": true,
        "configured": configured,
        "requires_reload": manifest.get("requiresReload").and_then(|v| v.as_bool()).unwrap_or(false),
        "config_yaml": redacted,
        "settings_schema": schema,
        "secret_fields": secret_keys,
        "secret_status": secret_status,
    });
    if let Some(fields) = out.as_object_mut() {
        super::agent_surface::insert_agent_fields(fields, &dir, &manifest);
    }
    Ok(out)
}

/// Grantable per-plugin permissions; an allowlist so a typo can't persist a bogus grant.
const GRANTABLE_PERMS: &[&str] = &[
    "project_write",
    "manage_workspaces",
    "spawn_agents",
    "agent_commands",
    "secrets",
    "graph_events",
];

/// `codeterm plugin config <id> [--set k=v]... [--secret FIELD] [--values-json '{…}'] [--grant P]... [--revoke P]...`.
pub(super) fn config_command(
    id: &str,
    sets: &[String],
    values_json: Option<&str>,
    secret: Option<(&str, &PluginSecretValue)>,
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

    if secret.is_some()
        && (!sets.is_empty() || values_json.is_some() || !grants.is_empty() || !revokes.is_empty())
    {
        return Err("--secret must be used by itself".to_string());
    }
    if let Some((field, _)) = secret {
        if !secret_keys.iter().any(|key| key == field) {
            return Err(format!(
                "--secret FIELD must name a secret field declared in {id}'s settings schema"
            ));
        }
    }

    let mut updates: Vec<(String, Value)> = Vec::new();
    for s in sets {
        let (k, v) = s
            .split_once('=')
            .ok_or_else(|| "--set expects KEY=VALUE".to_string())?;
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
    if updates.is_empty() && grants.is_empty() && revokes.is_empty() && secret.is_none() {
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

    for (key, _) in &updates {
        if secret_keys.contains(key) {
            return Err(format!(
                "setting '{key}' is secret; use `codeterm plugin config {id} --secret {key}` with the value piped to stdin"
            ));
        }
    }

    let yaml_path = config_yaml_path(&parsed_id, &dir);
    let mut text = match fs::read_to_string(&yaml_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("read {}: {e}", yaml_path.display())),
    };
    let mut changed: Vec<String> = Vec::new();
    for (k, v) in &updates {
        text = yaml_set_value(&text, k, v)?;
        changed.push(k.clone());
    }

    // Persist grant/revoke to grants.yaml; the host reads it fail-closed on next reload.
    let granted = apply_grants_for_settings(&dir, grants, revokes, &text)?;

    if !changed.is_empty() {
        write_atomic(&yaml_path, text.as_bytes())
            .map_err(|e| format!("write {}: {e}", yaml_path.display()))?;
    }

    let secret_fields_set = if let Some((field, value)) = secret {
        let bucket = plugin_secret_bucket(id);
        if !bucket.is_available() {
            return Err("plugin secret bucket is unavailable".to_string());
        }
        bucket
            .set(field, value.expose())
            .map_err(|error| format!("store secret field '{field}': {error}"))?;
        vec![field]
    } else {
        Vec::new()
    };

    Ok(serde_json::json!({
        "ok": true,
        "id": id,
        "changed": changed,
        "secret_fields_set": secret_fields_set,
        "grants_changed": granted,
        // config.yaml re-stamps live; grants apply on the plugin's next reload.
        "reload": if granted.is_empty() { "live" } else { "reload" },
        "config_path": yaml_path.display().to_string(),
    }))
}

fn config_yaml_path(id: &PluginId, dir: &Path) -> PathBuf {
    codeterm_util::plugin_config::config_yaml_path(&plugins_root(), id.as_str(), dir)
}

fn plugin_secret_bucket(plugin_id: &str) -> SecretFile {
    codeterm_util::secret_file::plugin_secret_bucket(data_root(), plugin_id)
}

pub(super) fn settings_command(
    id: &str,
    allow_hosts: &[String],
    disallow_hosts: &[String],
) -> Result<Value, String> {
    let id = PluginId::parse(id.to_string())?;
    let dir = plugin_dir(&id);
    if !dir.exists() {
        return Err(format!("plugin '{}' is not installed", id.as_str()));
    }
    if allow_hosts.is_empty() && disallow_hosts.is_empty() {
        return Err("nothing to do (use --allow-host or --disallow-host)".into());
    }
    let file = grants_file(&dir);
    let mut grants = match file.load(parse_grants) {
        Loaded::Parsed(grants) | Loaded::Recovered { value: grants, .. } => grants,
        Loaded::Missing if file.held().is_none() => Default::default(),
        Loaded::Missing | Loaded::Unrecoverable { .. } => {
            return Err(file
                .held()
                .map(|hold| hold.refusal())
                .unwrap_or_else(|| format!("{} failed to load", file.path().display())))
        }
    };
    let allow_hosts = normalize_hosts(allow_hosts)?;
    let disallow_hosts = normalize_hosts(disallow_hosts)?;
    for host in &allow_hosts {
        if !grants.network_allow.contains(host) {
            grants.network_allow.push(host.clone());
        }
    }
    grants
        .network_allow
        .retain(|host| !disallow_hosts.contains(host));
    grants.network_allow.sort();
    let body = serde_yaml::to_string(&grants).map_err(|e| format!("encode grants: {e}"))?;
    file.save(&body)?;
    Ok(serde_json::json!({
        "ok": true,
        "id": id.as_str(),
        "network_allow": grants.network_allow,
        "effective": "reload",
    }))
}

fn normalize_hosts(hosts: &[String]) -> Result<Vec<String>, String> {
    hosts
        .iter()
        .map(|host| codeterm_util::network_host::normalize_network_host(host))
        .collect()
}

/// Merge grant/revoke into the plugin's `grants.yaml` (a flat `{ permission: bool }`
/// map the host reads), validated against [`GRANTABLE_PERMS`]. Returns the changed names.
#[cfg(test)]
fn apply_grants(dir: &Path, grants: &[String], revokes: &[String]) -> Result<Vec<String>, String> {
    let settings = fs::read_to_string(dir.join("config.yaml")).unwrap_or_default();
    apply_grants_for_settings(dir, grants, revokes, &settings)
}

fn apply_grants_for_settings(
    dir: &Path,
    grants: &[String],
    revokes: &[String],
    settings: &str,
) -> Result<Vec<String>, String> {
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
        Loaded::Missing if file.held().is_none() => Default::default(),
        Loaded::Missing | Loaded::Unrecoverable { .. } => {
            return Err(file
                .held()
                .map(|hold| hold.refusal())
                .unwrap_or_else(|| format!("{} failed to load", file.path().display())))
        }
    };
    let mut changed = Vec::new();
    if grants.iter().any(|grant| grant == "project_write") {
        let manifest = read_plugin_manifest_value(dir)?;
        let selectors: Vec<String> = serde_json::from_value(
            manifest
                .pointer("/permissions/projectFiles")
                .cloned()
                .unwrap_or(Value::Null),
        )
        .map_err(|_| "plugin does not declare projectFiles selectors")?;
        let settings: Value = serde_yaml::from_str(settings).map_err(|e| e.to_string())?;
        let roots = codeterm_util::project_roots::configured(&settings, &selectors)?;
        if roots.is_empty() {
            return Err("configure an existing project root before granting project_write".into());
        }
        map.project_write_roots = roots
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
    }
    if revokes.iter().any(|grant| grant == "project_write") {
        map.project_write_roots.clear();
    }
    for p in grants {
        map.permissions.insert(p.clone(), true);
        changed.push(p.clone());
    }
    for p in revokes {
        map.permissions.insert(p.clone(), false);
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

fn parse_grants(text: &str) -> Result<codeterm_util::plugin_grants::PluginGrants, String> {
    if text.trim().is_empty() {
        return Ok(Default::default());
    }
    let grants: codeterm_util::plugin_grants::PluginGrants =
        serde_yaml::from_str(text).map_err(|e| e.to_string())?;
    for host in &grants.network_allow {
        codeterm_util::network_host::normalize_network_host(host)?;
    }
    Ok(grants)
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

/// Set a top-level `key` to `value`, preserving comments and unrelated keys.
/// The comment-preserving splice is accepted only when the result parses to exactly
/// the intended document; otherwise the whole document is re-serialized.
fn yaml_set_value(text: &str, key: &str, value: &Value) -> Result<String, String> {
    let original = parse_yaml_mapping(text);
    let spliced = splice_top_level(text, key, value);
    let accepted = match (&original, parse_yaml_mapping(&spliced)) {
        (Ok(before), Ok(after)) => {
            let mut expected = before.clone();
            expected.insert(key.to_string(), value.clone());
            after == expected
        }
        (Err(_), Ok(after)) => after.get(key) == Some(value),
        (_, Err(_)) => false,
    };
    if accepted {
        return Ok(spliced);
    }
    let mut map = original.map_err(|e| {
        format!("config.yaml is not valid YAML ({e}); fix or remove it, then retry")
    })?;
    map.insert(key.to_string(), value.clone());
    serde_yaml::to_string(&Value::Object(map)).map_err(|e| format!("encode config.yaml: {e}"))
}

fn parse_yaml_mapping(text: &str) -> Result<serde_json::Map<String, Value>, String> {
    if text.trim().is_empty() {
        return Ok(serde_json::Map::new());
    }
    let parsed: serde_yaml::Value = serde_yaml::from_str(text).map_err(|e| e.to_string())?;
    match serde_json::to_value(parsed).map_err(|e| e.to_string())? {
        Value::Null => Ok(serde_json::Map::new()),
        Value::Object(map) => Ok(map),
        _ => Err("top level is not a mapping".to_string()),
    }
}

/// Replace the key's whole value block (its line plus every continuation line), or append.
fn splice_top_level(text: &str, key: &str, value: &Value) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 1);
    let mut replaced = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        match top_level_key(line, key).filter(|_| !replaced) {
            Some(rest) => {
                let comment = rest.find(" #").map(|at| &rest[at..]).unwrap_or_default();
                out.push(render_entry(key, value, comment));
                replaced = true;
                i = value_block_end(&lines, i + 1);
            }
            None => {
                out.push(line.to_string());
                i += 1;
            }
        }
    }
    if !replaced {
        out.push(render_entry(key, value, ""));
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

/// Index just past the last continuation line of a top-level value starting at `start`.
/// Blank lines are consumed only when more of the block follows them.
fn value_block_end(lines: &[&str], start: usize) -> usize {
    let mut end = start;
    for (offset, line) in lines[start..].iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let sequence_item = *line == "-" || line.starts_with("- ") || line.starts_with("-\t");
        if !line.starts_with(char::is_whitespace) && !sequence_item {
            break;
        }
        end = start + offset + 1;
    }
    end
}

/// YAML for one top-level entry. Non-empty collections become an indented block
/// under `key:`; everything else stays on the key's line.
fn render_entry(key: &str, value: &Value, comment: &str) -> String {
    let encoded = encode_value(value);
    let nested = match value {
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
        _ => false,
    };
    if nested {
        let body: Vec<String> = encoded
            .lines()
            .map(|line| {
                if line.is_empty() {
                    String::new()
                } else {
                    format!("  {line}")
                }
            })
            .collect();
        return format!("{key}:{comment}\n{}", body.join("\n"));
    }
    match encoded.split_once('\n') {
        Some((head, tail)) => format!("{key}: {head}{comment}\n{tail}"),
        None => format!("{key}: {encoded}{comment}"),
    }
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

/// YAML-encode a typed value. Booleans/numbers emit natively (`false`, `8080`);
/// strings are quoted only when YAML requires it; collections emit block YAML.
fn encode_value(value: &Value) -> String {
    serde_yaml::to_string(value)
        .ok()
        .map(|s| s.trim_end().to_string())
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_scalar_replaces_value_preserving_comment() {
        let yaml = "# the speech engine\nengine: whisper # default\nport: 8080\n";
        let out = yaml_set_value(yaml, "engine", &infer_scalar("deepgram")).unwrap();
        assert!(out.contains("# the speech engine"), "leading comment kept");
        assert!(
            out.contains("engine: deepgram # default"),
            "inline comment kept: {out}"
        );
        assert!(out.contains("port: 8080"), "other lines kept");
    }

    #[test]
    fn set_scalar_appends_unknown_key() {
        let out =
            yaml_set_value("engine: whisper\n", "serverUrl", &infer_scalar("http://x")).unwrap();
        assert!(out.contains("engine: whisper"));
        assert!(out.contains("serverUrl: http://x"));
    }

    #[test]
    fn set_bool_writes_native_yaml_bool_not_string() {
        // `--set enabled=false` must write a real boolean, not the truthy string "false".
        let out = yaml_set_value("enabled: 'true'\n", "enabled", &infer_scalar("false")).unwrap();
        assert!(
            out.contains("enabled: false"),
            "native bool expected: {out}"
        );
        assert!(
            !out.contains("'false'") && !out.contains("\"false\""),
            "must not be a quoted string: {out}"
        );
    }

    fn endpoints_fixture() -> Value {
        serde_json::json!([
            {
                "name": "MiMo",
                "kind": "openai",
                "apiBase": "https://example.test/v1",
                "models": [{"id": "openai/mimo", "mapTokens": 8192}],
                "notes": "line one\nline two"
            },
            {"name": "Local", "kind": "openai", "models": []}
        ])
    }

    #[test]
    fn array_of_objects_round_trips_through_yaml_parser() {
        let yaml = "# Aider settings\nplainMode: false # bypass adapter\n";
        let endpoints = endpoints_fixture();
        let out = yaml_set_value(yaml, "endpoints", &endpoints).unwrap();
        let parsed = parse_yaml_mapping(&out).expect("valid YAML");
        assert_eq!(parsed["endpoints"], endpoints, "{out}");
        assert_eq!(parsed["plainMode"], false);
        assert!(out.contains("# Aider settings"));
        assert!(out.contains("plainMode: false # bypass adapter"));
        assert!(!out.contains("endpoints: -"), "{out}");
    }

    #[test]
    fn replacing_array_key_leaves_no_orphan_lines() {
        let yaml = "endpoints: # model endpoints\n  - name: Old\n    apiBase: https://old.test\n\n  - name: Older\n- name: ColumnZero\n\n# path doc\nconfigPath: /tmp/aider.yml\n";
        let endpoints = serde_json::json!([{"name": "New", "models": ["a"]}]);
        let out = yaml_set_value(yaml, "endpoints", &endpoints).unwrap();
        let parsed = parse_yaml_mapping(&out).expect("valid YAML");
        assert_eq!(parsed["endpoints"], endpoints, "{out}");
        assert_eq!(parsed["configPath"], "/tmp/aider.yml");
        assert_eq!(parsed.len(), 2, "{out}");
        for stale in ["Old", "old.test", "ColumnZero"] {
            assert!(!out.contains(stale), "orphan '{stale}' left: {out}");
        }
        assert!(out.contains("endpoints: # model endpoints"), "{out}");
        assert!(out.contains("\n\n# path doc\nconfigPath"), "{out}");
    }

    #[test]
    fn rewriting_a_broken_single_line_array_repairs_the_document() {
        let broken = "endpoints: - apiBase: https://old.test\n  kind: openai\nplainMode: true\n";
        assert!(parse_yaml_mapping(broken).is_err());
        let endpoints = endpoints_fixture();
        let out = yaml_set_value(broken, "endpoints", &endpoints).unwrap();
        let parsed = parse_yaml_mapping(&out).expect("valid YAML");
        assert_eq!(parsed["endpoints"], endpoints);
        assert_eq!(parsed["plainMode"], true);
    }

    #[test]
    fn replacing_array_with_scalar_drops_the_old_block() {
        let yaml = "endpoints:\n  - a\n  - b\nport: 1\n";
        let out = yaml_set_value(yaml, "endpoints", &serde_json::json!([])).unwrap();
        assert_eq!(out, "endpoints: []\nport: 1\n");
    }

    #[test]
    fn scalar_edit_leaves_nested_blocks_untouched() {
        let yaml =
            "decision:\n  model: x # pick\n  maxTokens: 64\nbaseUrl: http://localhost:1234\n";
        let out = yaml_set_value(yaml, "baseUrl", &infer_scalar("http://x")).unwrap();
        assert_eq!(
            out,
            "decision:\n  model: x # pick\n  maxTokens: 64\nbaseUrl: http://x\n"
        );
    }

    #[test]
    fn splice_that_cannot_match_falls_back_to_serializing_the_document() {
        let yaml = "\"endpoints\": [1]\nport: 2\n";
        let endpoints = serde_json::json!([{"name": "x"}]);
        let out = yaml_set_value(yaml, "endpoints", &endpoints).unwrap();
        let parsed = parse_yaml_mapping(&out).expect("valid YAML");
        assert_eq!(parsed["endpoints"], endpoints);
        assert_eq!(parsed["port"], 2);
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn unparseable_unrelated_yaml_is_refused_not_compounded() {
        let err = yaml_set_value("other: [\n", "port", &serde_json::json!(1)).unwrap_err();
        assert!(err.contains("not valid YAML"), "{err}");
    }

    #[test]
    fn values_json_config_writes_yaml_the_host_parses() {
        let root = tempfile::tempdir().unwrap();
        let id = "aider-fixture";
        let dir = root.path().join("plugins").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("plugin.json"),
            serde_json::json!({"id": id}).to_string(),
        )
        .unwrap();
        fs::write(dir.join("config.yaml"), "# docs\nplainMode: false\n").unwrap();
        let ctx = PluginCtx {
            data_root: root.path().to_path_buf(),
            version: None,
        };
        let endpoints = endpoints_fixture();
        let values = serde_json::json!({"endpoints": endpoints, "configPath": "/a b"});
        for _ in 0..2 {
            crate::run(
                ctx.clone(),
                PluginAction::Config {
                    id: id.into(),
                    set: Vec::new(),
                    values_json: Some(values.to_string()),
                    secret: None,
                    grant: Vec::new(),
                    revoke: Vec::new(),
                },
            )
            .unwrap();
        }
        let text = fs::read_to_string(dir.join("config.yaml")).unwrap();
        let parsed = parse_yaml_mapping(&text).expect("valid YAML");
        assert_eq!(parsed["endpoints"], endpoints, "{text}");
        assert_eq!(parsed["configPath"], "/a b");
        assert_eq!(parsed["plainMode"], false);
        assert!(text.starts_with("# docs\n"));
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
    fn project_write_grant_snapshots_configured_roots_and_revokes() {
        let dir = temp_dir();
        let project = temp_dir();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"permissions":{"projectFiles":["/root"]}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("config.yaml"),
            serde_yaml::to_string(&serde_json::json!({"root":project})).unwrap(),
        )
        .unwrap();
        apply_grants(&dir, &["project_write".into()], &[]).unwrap();
        let granted =
            parse_grants(&std::fs::read_to_string(dir.join("grants.yaml")).unwrap()).unwrap();
        assert_eq!(
            granted.project_write_roots,
            [project.canonicalize().unwrap().to_string_lossy()]
        );
        assert_eq!(granted.permissions.get("project_write"), Some(&true));
        apply_grants(&dir, &[], &["project_write".into()]).unwrap();
        assert!(
            parse_grants(&std::fs::read_to_string(dir.join("grants.yaml")).unwrap())
                .unwrap()
                .project_write_roots
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(project);
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

    #[test]
    fn secret_config_uses_plugin_bucket_and_reports_only_presence() {
        let root = tempfile::tempdir().unwrap();
        let id = "secret-fixture";
        let dir = root.path().join("plugins").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("plugin.json"),
            serde_json::json!({
                "id": id,
                "settingsSchema": "settings.schema.json"
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            dir.join("settings.schema.json"),
            serde_json::json!([{
                "kind": "api_key",
                "env_var": "SERVICE_TOKEN",
                "label": "Service token"
            }])
            .to_string(),
        )
        .unwrap();
        let ctx = PluginCtx {
            data_root: root.path().to_path_buf(),
            version: None,
        };
        let unset = crate::run(ctx.clone(), PluginAction::Get { id: id.into() }).unwrap();
        assert_eq!(unset["secret_status"]["SERVICE_TOKEN"], "unset");

        let value = format!("fixture-{}", uuid::Uuid::new_v4());
        let mut stdin_value = value.as_bytes();
        let secret_input = PluginSecretValue::read_from(&mut stdin_value, false).unwrap();
        assert!(!format!("{secret_input:?}").contains(value.as_str()));
        let configured = crate::run(
            ctx.clone(),
            PluginAction::Config {
                id: id.into(),
                set: Vec::new(),
                values_json: None,
                secret: Some(("SERVICE_TOKEN".into(), secret_input)),
                grant: Vec::new(),
                revoke: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(configured["secret_fields_set"][0], "SERVICE_TOKEN");
        assert!(!configured.to_string().contains(value.as_str()));
        let host_bucket = codeterm_util::secret_file::plugin_secret_bucket(root.path(), id);
        let stored = host_bucket.get("SERVICE_TOKEN").unwrap();
        assert!(stored.as_str() == value.as_str());
        let other_bucket = codeterm_util::secret_file::plugin_secret_bucket(root.path(), "other");
        assert!(other_bucket.get("SERVICE_TOKEN").is_err());

        let shown = crate::run(ctx.clone(), PluginAction::Get { id: id.into() }).unwrap();
        assert_eq!(shown["secret_status"]["SERVICE_TOKEN"], "set");
        assert!(!shown.to_string().contains(value.as_str()));
        assert!(!dir.join("config.yaml").exists());

        let set_error = crate::run(
            ctx,
            PluginAction::Config {
                id: id.into(),
                set: vec![format!("SERVICE_TOKEN={stored}")],
                values_json: None,
                secret: None,
                grant: Vec::new(),
                revoke: Vec::new(),
            },
        )
        .unwrap_err();
        assert!(set_error.contains("--secret SERVICE_TOKEN"));
        assert!(!set_error.contains(stored.as_str()));
        let audit = fs::read_to_string(root.path().join("logs/plugin-events.log")).unwrap();
        assert!(!audit.contains(stored.as_str()));
    }

    #[test]
    fn get_returns_the_declared_verbs_and_bundled_agent_guide() {
        let root = tempfile::tempdir().unwrap();
        let id = "guide-fixture";
        let dir = root.path().join("plugins").join(id);
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(
            dir.join("plugin.json"),
            serde_json::json!({
                "id": id,
                "agentVerbs": ["zz-alpha"],
                "agentGuide": "docs/AGENT.md"
            })
            .to_string(),
        )
        .unwrap();
        fs::write(dir.join("docs/AGENT.md"), "guide-token-q7").unwrap();
        let ctx = PluginCtx {
            data_root: root.path().to_path_buf(),
            version: None,
        };
        let shown = crate::run(ctx, PluginAction::Get { id: id.into() }).unwrap();
        assert_eq!(shown["agent_verbs"], serde_json::json!(["zz-alpha"]));
        assert_eq!(shown["agent_guide"]["text"], "guide-token-q7");
    }

    #[test]
    fn settings_persists_only_validated_exact_host_grants() {
        let root = temp_dir();
        let _ctx = enter_ctx(PluginCtx {
            data_root: root.clone(),
            version: None,
        });
        let id = PluginId::parse("local-model").unwrap();
        let dir = plugin_dir(&id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("grants.yaml"), "spawn_agents: true\n").unwrap();

        settings_command("local-model", &["API.Example.test:8443".into()], &[]).unwrap();
        let body = fs::read_to_string(dir.join("grants.yaml")).unwrap();
        let grants = parse_grants(&body).unwrap();
        assert_eq!(grants.permissions.get("spawn_agents"), Some(&true));
        assert_eq!(grants.network_allow, ["api.example.test:8443"]);

        assert!(settings_command("local-model", &["https://example.test".into()], &[]).is_err());
        assert!(settings_command("local-model", &["*.example.test".into()], &[]).is_err());
        let after_refusals = fs::read_to_string(dir.join("grants.yaml")).unwrap();
        assert_eq!(
            parse_grants(&after_refusals).unwrap().network_allow,
            ["api.example.test:8443"]
        );
        let _ = fs::remove_dir_all(root);
    }
}
