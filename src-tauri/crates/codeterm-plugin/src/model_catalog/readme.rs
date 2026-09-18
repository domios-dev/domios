//! The catalog's README + `<provider>.example.toml` writer. Split out of
//! `model_catalog.rs` to keep that file under the source-size limit.

use super::*;

/// Auto-install the README documenting the catalog format. Called once on
/// startup so the user discovers the directory naturally.
pub fn ensure_readme_installed() {
    let dir = models_dir();
    let readme = dir.join("README.md");
    if readme.exists() {
        return;
    }
    if let Err(e) = std::fs::write(&readme, README_BODY) {
        log::warn!(
            "[model_catalog] could not write {}: {}",
            readme.display(),
            e
        );
    }
}

/// Refresh `<id>.example.toml` for every registered provider. The .example
/// files are ALWAYS overwritten so they reflect the current build's
/// defaults — that's their whole point. The user's `<id>.toml` (no
/// `.example`) is the override and is never touched here. To enable an
/// override the user copies `claude.example.toml` → `claude.toml`,
/// edits, and saves.
pub fn refresh_example_files() {
    let dir = models_dir();
    let registered_efforts = effort_registry().read().clone();
    let snapshot: Vec<(String, RegisteredDefaults, RegisteredEfforts)> = seed_registry()
        .read()
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                v.clone(),
                registered_efforts.get(k).cloned().unwrap_or_default(),
            )
        })
        .collect();
    for (id, defaults, efforts) in snapshot {
        let path = dir.join(format!("{}.example.toml", id));
        let body = render_example(&id, &defaults, &efforts);
        if let Err(e) = std::fs::write(&path, body) {
            log::warn!("[model_catalog] could not write {}: {}", path.display(), e);
        }
    }
}

pub(crate) fn render_example(
    provider_id: &str,
    defaults: &RegisteredDefaults,
    effort_defaults: &RegisteredEfforts,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Example model catalog for `{0}`.\n\
         #\n\
         # To enable an override, copy this file to `{0}.toml` (drop the\n\
         # `.example` suffix). CodeTerm picks up `{0}.toml` on the next read\n\
         # — no restart. Delete it to fall back to built-in defaults.\n\
         #\n\
         # ## Merge semantics (default)\n\
         #\n\
         # By default, `[[models]]` and `[[efforts]]` MERGE with built-ins by id:\n\
         #   - Same id          → your entry replaces the built-in (keeps order).\n\
         #   - New id           → your entry is APPENDED at the end.\n\
         #   - Built-in id you didn't mention → stays, auto-updates on release.\n\
         #\n\
         # That means: adding one custom model doesn't freeze the rest of\n\
         # your list — when a new CodeTerm release ships an updated built-in\n\
         # catalog, it arrives automatically alongside your customizations.\n\
         #\n\
         # ## Full replacement\n\
         #\n\
         # If you want absolute control and no built-in updates ever, set\n\
         # `replace = true` at the top. Then this file is the entire list,\n\
         # and built-ins are ignored.\n\
         #\n\
         # This `.example` file is regenerated on every CodeTerm start so you\n\
         # can always see what the current build ships with.\n\n",
        provider_id
    ));
    out.push_str("# replace = true   # uncomment to disable merge with built-ins\n\n");
    if let Some(d) = &defaults.default {
        out.push_str(&format!("default = \"{}\"\n\n", d));
    }
    if let Some(default) = &effort_defaults.default {
        out.push_str(&format!("default_effort = \"{}\"\n\n", default));
    }
    for m in &defaults.models {
        out.push_str("[[models]]\n");
        out.push_str(&format!("id = \"{}\"\n", m.id));
        out.push_str(&format!(
            "display_name = \"{}\"\n",
            m.display_name.replace('"', "\\\"")
        ));
        if let Some(desc) = &m.description {
            out.push_str(&format!(
                "description = \"{}\"\n",
                desc.replace('"', "\\\"")
            ));
        }
        if m.is_featured() {
            out.push_str("featured = true\n");
        }
        if let Some(badge) = &m.badge {
            out.push_str(&format!("badge = \"{}\"\n", badge.replace('"', "\\\"")));
        }
        out.push('\n');
    }
    for effort in &effort_defaults.efforts {
        out.push_str("[[efforts]]\n");
        out.push_str(&format!("id = \"{}\"\n", effort.id));
        out.push_str(&format!(
            "display_name = \"{}\"\n",
            effort.display_name.replace('"', "\\\"")
        ));
        if let Some(description) = &effort.description {
            out.push_str(&format!(
                "description = \"{}\"\n",
                description.replace('"', "\\\"")
            ));
        }
        out.push('\n');
    }
    out
}

const README_BODY: &str = r#"# CodeTerm Model Catalog

This folder lets you override the model list for any agent provider without
rebuilding CodeTerm. Useful when upstream ships new models faster than we cut
releases.

## Quick start

For every provider with a model picker, CodeTerm drops a
`<provider>.example.toml` here on each startup. To enable an override:

```bash
cd ~/.codeterm/models/
cp claude.example.toml claude.toml
# edit claude.toml — change models or reasoning efforts, set defaults
```

CodeTerm picks `claude.toml` up immediately (no restart). Static provider
catalogs merge overrides by default; dynamic providers may treat an override as
a pinned list. Delete `claude.toml` (the override; NOT the `.example` file) to
fall back to provider defaults.

## Files

- `claude.toml`, `codex.toml`, `opencode.toml` — **your overrides** (only
  present if you copied the example). Never touched by CodeTerm.
- `claude.example.toml`, etc. — **always-current built-ins**, rewritten on
  every startup so you can see what ships with this build.
- `README.md` — this file. Auto-installed on first run.

## Format

```toml
# Optional: which model is selected by default for fresh sessions.
default = "example-model"
default_effort = "medium"

[[models]]
id = "example-model"
display_name = "Example Model (extended context)"
description = "Best for everyday, complex tasks."
featured = true
badge = "own"

[[models]]
id = "example-model-mini"
display_name = "Example Model Mini"
description = "Efficient for routine tasks."

[[efforts]]
id = "xhigh"
display_name = "Extra high"
description = "Maximum reasoning depth."
```

- `id` MUST match what the underlying CLI accepts (`claude --model <id>`,
  `codex -m <id>`, `opencode --model <provider>/<model>`). Wrong ids cause
  launch failures.
- `display_name` is what the picker UI shows.
- `description` is optional secondary text.
- `featured = true` lists this model ahead of the rest, preserving catalog
  order within each group.
- `badge` is an optional short chip shown beside the model in spawn pickers.
- `[[efforts]]` entries extend or replace accepted reasoning-effort ids using
  the same merge rules as models.

## Merge vs replace

By default, your override **merges** with built-ins by id:
- Same id      → your entry replaces the built-in (keeps original order).
- New id       → your entry is appended at the end.
- Built-in id you didn't mention → stays. Auto-updates on each CodeTerm
  release.

When a provider reports a live catalog (its own CLI probe), that catalog is the
set of spawnable models: your override may decorate an id the provider reports
(featured, badge, display name) and order it, but it can neither hide a reported
model nor add one the provider does not have. A model you never listed here is
still spawnable the moment the provider offers it.

If you want absolute control and no built-in updates ever, set
`replace = true` at the top of your `<provider>.toml`. Then your file is
the entire list, period.

## Lookup precedence

1. `<this dir>/<provider>.toml` (override) merged with built-ins, OR replacing
   them entirely if `replace = true`.
2. Provider defaults: either the provider's live CLI catalog when available,
   or plugin defaults shipped with the current CodeTerm build.

Syntax error or no `[[models]]` / `[[efforts]]` entries? CodeTerm logs a warning
and falls back to built-ins — the picker never goes empty.

## Full docs

See `essences/models.md` in the CodeTerm repository for the long-form reference
(plugin contract, how to add a new provider, etc.).
"#;
