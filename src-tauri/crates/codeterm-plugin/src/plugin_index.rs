use std::collections::HashMap;

use serde_json::Value;

use crate::agent_verbs::{plugin_skill_title, AgentVerbCatalog, PLUGIN_SKILL_SOURCE_PREFIX};
use crate::js_runtime::PluginManifest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginIndexEntry {
    pub plugin_id: String,
    pub title: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub body: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingPluginIndexEntry {
    pub mem_id: String,
    pub title: String,
    pub source: String,
    pub body: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PluginIndexPlan {
    pub upserts: Vec<PluginIndexEntry>,
    pub removals: Vec<String>,
}

pub fn entries_from_manifest(
    manifest: &PluginManifest,
    settings_schema: Option<&Value>,
    guide: Option<&str>,
) -> Vec<PluginIndexEntry> {
    let catalog = AgentVerbCatalog::compose(
        &manifest.id,
        &manifest.display_name,
        manifest.description.as_deref(),
        &manifest.agent_verbs,
        manifest.config_help.as_deref(),
        &manifest.agent_keywords,
    )
    .with_guide(guide.map(str::to_owned));
    entries_from_catalog(manifest, settings_schema, guide, &catalog)
}

pub fn entries_from_catalog(
    manifest: &PluginManifest,
    settings_schema: Option<&Value>,
    guide: Option<&str>,
    catalog: &AgentVerbCatalog,
) -> Vec<PluginIndexEntry> {
    let capabilities = serde_json::to_value(&manifest.capabilities).unwrap_or(Value::Null);
    let capability_lines = capability_lines(&capabilities);
    let surfaces = surface_lines(&capabilities);
    let settings = settings_lines(settings_schema);
    let mut entries = vec![plugin_entry(
        manifest,
        &capability_lines,
        &surfaces,
        &settings,
        catalog,
    )];
    let mut slug_counts = HashMap::new();
    for usage in &catalog.verbs {
        *slug_counts.entry(slug(usage)).or_insert(0usize) += 1;
    }
    entries.extend(catalog.verbs.iter().map(|usage| {
        let base_slug = slug(usage);
        let key = if slug_counts.get(&base_slug).copied().unwrap_or_default() > 1 {
            format!("{base_slug}-{:016x}", stable_hash(usage))
        } else {
            base_slug
        };
        verb_entry(manifest, usage, guide, &key)
    }));
    let recall = crate::plugin_connection::recall_line(catalog);
    for entry in &mut entries {
        entry.body = format!("{recall}\n\n{}", entry.body);
        entry.summary = format!("{recall} {}", entry.summary);
    }
    entries
}

pub fn plan_plugin_index_sync(
    desired: &[PluginIndexEntry],
    existing: &[ExistingPluginIndexEntry],
) -> PluginIndexPlan {
    let mut desired = desired.to_vec();
    desired.sort_by(|a, b| a.source.cmp(&b.source));
    desired.dedup_by(|a, b| a.source == b.source);
    let removals = existing
        .iter()
        .filter(|item| {
            !desired
                .iter()
                .any(|entry| entry.source == item.source && entry.title == item.title)
        })
        .map(|item| item.mem_id.clone())
        .collect();
    let upserts = desired
        .into_iter()
        .filter(|entry| {
            !existing.iter().any(|item| {
                item.source == entry.source
                    && item.title == entry.title
                    && item.body.trim() == entry.body.trim()
            })
        })
        .collect();
    PluginIndexPlan { upserts, removals }
}

fn plugin_entry(
    manifest: &PluginManifest,
    capabilities: &[String],
    surfaces: &[String],
    settings: &[String],
    catalog: &AgentVerbCatalog,
) -> PluginIndexEntry {
    let id = &manifest.id;
    let name = &manifest.display_name;
    let source = format!("{PLUGIN_SKILL_SOURCE_PREFIX}{id}");
    let mut body = format!("# {name} plugin (`{id}`)\n\n");
    if let Some(description) = manifest
        .description
        .as_deref()
        .filter(|text| !text.trim().is_empty())
    {
        body.push_str(description.trim());
        body.push_str("\n\n");
    }
    body.push_str(&format!(
        "Installed plugin. Inspect its current configuration and agent guidance with `domios plugin get {id}`.\n"
    ));
    if !catalog.verbs.is_empty() {
        body.push_str("\nAgent verbs:\n");
        for usage in &catalog.verbs {
            body.push_str(&format!("- `domios plugin {id} {usage}`\n"));
        }
    }
    if !capabilities.is_empty() {
        body.push_str(&format!("\nCapabilities:\n{}\n", bullets(capabilities)));
    }
    if !surfaces.is_empty() {
        body.push_str(&format!("\nUI surfaces:\n{}\n", bullets(surfaces)));
    }
    if !settings.is_empty() {
        body.push_str(&format!("\nConfiguration keys:\n{}\n", bullets(settings)));
    }
    if let Some(help) = manifest
        .config_help
        .as_deref()
        .filter(|text| !text.trim().is_empty())
    {
        body.push_str("\nConfiguration help:\n");
        body.push_str(help.trim());
        body.push('\n');
    }

    let mut tags = plugin_tags(manifest);
    tags.push("plugin".into());
    tags.sort();
    tags.dedup();
    PluginIndexEntry {
        plugin_id: id.clone(),
        title: plugin_skill_title(id),
        summary: format!(
            "{name} plugin: {}",
            manifest
                .description
                .as_deref()
                .unwrap_or("installed capabilities and settings")
        ),
        tags,
        body,
        source,
    }
}

fn verb_entry(
    manifest: &PluginManifest,
    usage: &str,
    guide: Option<&str>,
    key: &str,
) -> PluginIndexEntry {
    let id = &manifest.id;
    let name = &manifest.display_name;
    let verb = usage.split_whitespace().next().unwrap_or(usage);
    let invocation = format!("domios plugin {id} {usage}");
    let args = usage
        .split_once(char::is_whitespace)
        .map(|(_, args)| args.trim())
        .filter(|args| !args.is_empty())
        .unwrap_or("none");
    let description = guide_description(guide.unwrap_or_default(), verb)
        .or_else(|| manifest.config_help.clone())
        .or_else(|| manifest.description.clone())
        .unwrap_or_else(|| "No additional verb description is declared.".into());
    let mut body = format!(
        "# {name} {verb} plugin verb\n\nPlugin: `{id}`\n\nVerb usage: `{usage}`\n\nArguments: `{args}`\n\nDescription:\n{description}\n\nInvocation: `{invocation}`\n"
    );
    if let Some(help) = manifest
        .config_help
        .as_deref()
        .filter(|text| !text.trim().is_empty() && !description.contains(text.trim()))
    {
        body.push_str("\nPlugin help:\n");
        body.push_str(help.trim());
        body.push('\n');
    }
    let mut tags = plugin_tags(manifest);
    tags.extend(
        verb.split(|ch: char| !ch.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase),
    );
    tags.push("plugin".into());
    tags.push("agent-command".into());
    tags.sort();
    tags.dedup();
    PluginIndexEntry {
        plugin_id: id.clone(),
        title: format!("{}-{key}", plugin_skill_title(id)),
        summary: format!("{name} {verb}: {description}"),
        tags,
        body,
        source: format!("{PLUGIN_SKILL_SOURCE_PREFIX}{id}:verb:{key}"),
    }
}

fn plugin_tags(manifest: &PluginManifest) -> Vec<String> {
    let mut tags = if manifest.agent_keywords.is_empty() {
        [
            manifest.display_name.as_str(),
            manifest.description.as_deref().unwrap_or(""),
        ]
        .into_iter()
        .flat_map(|text| text.split(|ch: char| !ch.is_alphanumeric()))
        .filter(|word| word.chars().count() >= 3)
        .map(str::to_lowercase)
        .collect()
    } else {
        manifest.agent_keywords.clone()
    };
    tags.extend(
        manifest
            .display_name
            .split(|ch: char| !ch.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase),
    );
    tags.push(manifest.id.clone());
    tags
}

fn capability_lines(value: &Value) -> Vec<String> {
    fn collect(value: &Value, path: &mut Vec<String>, out: &mut Vec<String>) {
        match value {
            Value::Object(fields) => {
                for (key, child) in fields {
                    path.push(readable_key(key));
                    collect(child, path, out);
                    path.pop();
                }
            }
            Value::Bool(enabled) => out.push(format!(
                "{} {}",
                path.join(" "),
                if *enabled { "enabled" } else { "disabled" }
            )),
            Value::String(value) if !value.trim().is_empty() => {
                out.push(format!("{}: {}", path.join(" "), value.trim()))
            }
            _ => {}
        }
    }

    let mut out = Vec::new();
    collect(value, &mut Vec::new(), &mut out);
    out.sort();
    out.dedup();
    out
}

fn surface_lines(capabilities: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(surfaces) = capabilities.get("surfaces").and_then(Value::as_array) {
        for surface in surfaces {
            let id = surface
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("surface");
            let title = surface.get("title").and_then(Value::as_str).unwrap_or(id);
            let renderer = surface
                .get("renderer")
                .and_then(Value::as_str)
                .unwrap_or("view");
            let runtime = surface
                .get("runtime")
                .and_then(Value::as_str)
                .map(|runtime| format!(", runtime {runtime}"))
                .unwrap_or_default();
            lines.push(format!("{id} ({title}, {renderer}{runtime})"));
        }
    }
    if let Some(items) = capabilities.get("tabMenu").and_then(Value::as_array) {
        for item in items {
            let label = item.get("label").and_then(Value::as_str);
            let action = item.get("id").and_then(Value::as_str);
            if let Some(label) = label {
                lines.push(format!(
                    "tab menu: {label}{}",
                    action.map(|a| format!(" ({a})")).unwrap_or_default()
                ));
            }
        }
    }
    lines.sort();
    lines.dedup();
    lines
}

fn settings_lines(schema: Option<&Value>) -> Vec<String> {
    fn collect(value: &Value, prefix: &str, out: &mut Vec<String>) {
        match value {
            Value::Array(items) => {
                for item in items {
                    collect(item, prefix, out);
                }
            }
            Value::Object(fields) => {
                let kind = fields.get("kind").and_then(Value::as_str).unwrap_or("");
                let raw_key = fields
                    .get("key")
                    .or_else(|| (kind == "api_key").then(|| fields.get("envVar")).flatten())
                    .and_then(Value::as_str);
                let key = raw_key.map(|key| {
                    if prefix.is_empty() {
                        key.to_owned()
                    } else {
                        format!("{prefix}.{key}")
                    }
                });
                if let Some(key) = &key {
                    let label = fields.get("label").and_then(Value::as_str).unwrap_or("");
                    let description = fields
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let detail = [label, description]
                        .into_iter()
                        .filter(|part| !part.trim().is_empty())
                        .collect::<Vec<_>>()
                        .join(": ");
                    let secret = if kind == "api_key" { " (secret)" } else { "" };
                    if detail.is_empty() {
                        out.push(format!("{key}{secret}"));
                    } else {
                        out.push(format!("{key}{secret}: {detail}"));
                    }
                } else if kind == "section" {
                    if let Some(title) = fields.get("title").and_then(Value::as_str) {
                        let description = fields
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        out.push(format!("settings section {title}: {description}"));
                    }
                }
                if let Some(children) = fields.get("fields") {
                    collect(children, key.as_deref().unwrap_or(prefix), out);
                }
                if let Some(item) = fields.get("item") {
                    let item_prefix = key
                        .as_deref()
                        .map(|key| format!("{key}[]"))
                        .unwrap_or_else(|| prefix.to_owned());
                    collect(item, &item_prefix, out);
                }
            }
            _ => {}
        }
    }

    let mut out = Vec::new();
    if let Some(schema) = schema {
        collect(schema, "", &mut out);
    }
    out.sort();
    out.dedup();
    out
}

fn guide_description(guide: &str, verb: &str) -> Option<String> {
    let mut matching = None;
    let mut section_level = 0usize;
    let mut lines = Vec::new();
    let verb = verb.to_lowercase();
    for line in guide.lines() {
        let trimmed = line.trim_start();
        let level = trimmed.chars().take_while(|ch| *ch == '#').count();
        let is_heading = (1..=6).contains(&level) && trimmed.as_bytes().get(level) == Some(&b' ');
        if is_heading {
            if section_level > 0 && level <= section_level {
                break;
            }
            if section_level == 0 {
                let title = trimmed[level..].trim().trim_end_matches('#').to_lowercase();
                if title.split(|ch: char| !ch.is_alphanumeric()).any(|word| {
                    word == verb
                        || word.strip_suffix("ing") == Some(verb.as_str())
                        || word.strip_suffix("ed") == Some(verb.as_str())
                        || word.strip_suffix('s') == Some(verb.as_str())
                }) {
                    matching = Some(line.to_string());
                    section_level = level;
                    lines.push(line.to_string());
                    continue;
                }
            }
        }
        if section_level > 0 {
            lines.push(line.to_string());
        }
    }
    let description = lines.join("\n").trim().to_string();
    (matching.is_some() && !description.is_empty()).then_some(description)
}

fn readable_key(key: &str) -> String {
    let mut readable = String::new();
    for character in key.chars() {
        if character == '_' || character == '-' {
            readable.push(' ');
        } else if character.is_uppercase() {
            readable.push(' ');
            readable.extend(character.to_lowercase());
        } else {
            readable.push(character);
        }
    }
    readable.trim().to_string()
}

fn slug(value: &str) -> String {
    let mut slug = String::new();
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            slug.push(ch);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_string()
}

fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn bullets(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| format!("- {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
