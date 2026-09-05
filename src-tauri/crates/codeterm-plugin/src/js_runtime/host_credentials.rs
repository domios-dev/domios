//! Host-side credential reads for confined plugins.
//!
//! A plugin declares a credential file in its manifest instead of reading it.
//! The host resolves the file: non-secret fields come back through
//! `host.credentialPublic`, secret fields are only ever substituted into an
//! outbound `host.fetch` header template and never cross into the VM. The
//! declared file is simultaneously removed from the plugin's read roots
//! (`PluginSandbox::denied_reads`), so declaring a credential is what takes
//! direct access to it away.

use std::collections::BTreeMap;

use serde_json::Value;

use super::manifest::{CredentialConfig, PluginManifest};

/// Resolve a dotted path (`tokens.access_token`) inside a JSON document.
fn value_at<'a>(doc: &'a Value, dotted: &str) -> Option<&'a Value> {
    dotted
        .split('.')
        .try_fold(doc, |node, key| node.get(key))
        .filter(|v| !v.is_null())
}

fn read_document(cred: &CredentialConfig) -> Option<Value> {
    let path = PluginManifest::expand_home(&cred.file)?;
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&text).ok()
}

fn resolve_fields(doc: &Value, spec: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (name, dotted) in spec {
        if let Some(text) = value_at(doc, dotted).and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        }) {
            out.insert(name.clone(), text);
        }
    }
    out
}

/// Absolute paths of every declared credential file — the sandbox's read deny-list.
pub(super) fn declared_files(credentials: &[CredentialConfig]) -> Vec<std::path::PathBuf> {
    credentials
        .iter()
        .filter_map(|c| PluginManifest::expand_home(&c.file))
        .collect()
}

/// `host.credentialPublic(id)` — the non-secret fields only, as a JSON object.
/// `None` when the credential is undeclared, the file is unreadable, or no
/// public field resolved (an honest absence, never a partial guess).
pub(super) fn public_json(credentials: &[CredentialConfig], id: &str) -> Option<String> {
    let cred = credentials.iter().find(|c| c.id == id)?;
    let doc = read_document(cred)?;
    let public = resolve_fields(&doc, &cred.public);
    if public.is_empty() {
        return None;
    }
    serde_json::to_string(&public).ok()
}

/// `{field}` placeholders a header template may carry.
fn substitute(template: &str, fields: &BTreeMap<String, String>) -> Option<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}')? + open;
        out.push_str(&rest[..open]);
        out.push_str(fields.get(&rest[open + 1..close])?);
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// Rewrite `host.fetch` opts so a declared credential's secret fields land in
/// request headers without the plugin ever holding them.
///
/// Opts shape: `"credential": { "id": "...", "headers": { ... } }`, where each
/// header value is a template such as `"Bearer {accessToken}"`.
/// The `credential` key is stripped from the outgoing opts. `Err` when the
/// credential is declared-but-unresolvable, so a missing token surfaces as a
/// fetch error instead of an unauthenticated request.
pub(super) fn inject(credentials: &[CredentialConfig], opts_json: &str) -> Result<String, String> {
    let mut opts: Value =
        serde_json::from_str(opts_json).map_err(|e| format!("bad fetch opts: {e}"))?;
    let Some(request) = opts.as_object_mut().and_then(|o| o.remove("credential")) else {
        return Ok(opts_json.to_string());
    };
    let id = request
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "credential request has no id".to_string())?;
    let cred = credentials
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("credential {id} is not declared in the manifest"))?;
    let doc = read_document(cred).ok_or_else(|| format!("credential {id} is unreadable"))?;
    let mut fields = resolve_fields(&doc, &cred.public);
    fields.extend(resolve_fields(&doc, &cred.secret));

    let templates = request
        .get("headers")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let object = opts
        .as_object_mut()
        .ok_or_else(|| "fetch opts is not an object".to_string())?;
    let headers = object
        .entry("headers".to_string())
        .or_insert_with(|| Value::Object(Default::default()));
    let headers = headers
        .as_object_mut()
        .ok_or_else(|| "fetch headers is not an object".to_string())?;
    for (name, template) in templates {
        let template = template
            .as_str()
            .ok_or_else(|| format!("credential header {name} is not a string"))?;
        let filled = substitute(template, &fields)
            .ok_or_else(|| format!("credential {id} cannot fill header {name}"))?;
        headers.insert(name, Value::String(filled));
    }
    serde_json::to_string(&opts).map_err(|e| format!("serialize fetch opts: {e}"))
}

#[cfg(test)]
mod tests;
