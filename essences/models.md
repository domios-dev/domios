# Agent Model Catalog

How CodeTerm decides what models and reasoning efforts to show in agent pickers,
Settings → Agents, and launch flags.

## Architecture

Each agent provider plugin owns its model list. Provider lists can be live
when the upstream CLI exposes a stable catalog command. Two layers:

```
┌─────────────────────────────────────────────────┐
│   user override at <data_dir>/models/<id>.toml  │   ← edit this
├─────────────────────────────────────────────────┤
│   provider list (live CLI or shipped fallback)  │   ← provider-owned source
└─────────────────────────────────────────────────┘
```

Static provider catalogs **merge** by default — see [Merge vs replace](#merge-vs-replace)
below. Dynamic providers such as Codex/OpenCode treat an override file as an
explicit pinned list, so they do not mix local overrides with a live account-
dependent catalog.

Where `<data_dir>` is:
- Prod: `~/.codeterm/`
- Dev: `~/.codeterm-dev/`

## Merge vs replace

The conflict CodeTerm solves: you add a custom model. Two months later a
new release ships `opus 4.8`. Without merge, your override file freezes
the list at install time and you never see `opus 4.8`. With merge (default),
both arrive together.

### Default: merge by id

Without any extra flag, the override file MERGES with built-ins by id:

| Case | Result |
|------|--------|
| Override `[[models]] id = "opus"` matches a built-in `opus` | Your entry **replaces** the built-in (display name, description). Original list position is kept. |
| Override `[[models]] id = "my-custom"` (no built-in match) | Your entry is **appended** at the end of the list. |
| Built-in id you don't mention | **Stays**. Auto-updates on each release. |

#### Worked example

You install CodeTerm shipping these claude defaults:

```toml
[[models]]
id = "opus"
display_name = "Claude Opus 4.7 (1M context)"

[[models]]
id = "sonnet"
display_name = "Claude Sonnet 4.6"

[[models]]
id = "haiku"
display_name = "Claude Haiku 4.5"
```

You add a custom finetune in `~/.codeterm/models/claude.toml`:

```toml
default = "sonnet"

[[models]]
id = "sonnet-min-1"
display_name = "Claude Sonnet Mini (my finetune)"
description = "Routed through my proxy."
```

Visible models: `opus`, `sonnet`, `haiku`, `sonnet-min-1` (built-in order
preserved, your custom entry appended).

Two months later CodeTerm 1.4 ships:

```toml
[[models]]
id = "opus"
display_name = "Claude Opus 4.8 (1M context)"   # ← bumped

[[models]]
id = "sonnet"
display_name = "Claude Sonnet 4.7"              # ← bumped

[[models]]
id = "haiku"
display_name = "Claude Haiku 4.5"
```

After upgrade, your visible list automatically becomes: `opus 4.8`,
`sonnet 4.7`, `haiku 4.5`, `sonnet-min-1`. No edits needed — built-ins
flowed through, your custom entry survived.

### `replace = true`: absolute control, no upgrades

If you want to pin your list **exactly** and never accept upstream model
updates, put `replace = true` at the top:

```toml
replace = true
default = "opus"

[[models]]
id = "opus"
display_name = "Claude Opus 4.7 (1M context)"
# ... only entries listed here will ever appear
```

In replace mode, built-ins are ignored entirely. New CodeTerm releases
shipping `opus 4.8` won't show up — you'd need to edit the file by hand.
Use this when stability matters more than freshness (e.g. air-gapped
deployments, model-locked workflows).

### Gotcha: owning a built-in id freezes that one entry

When your `claude.toml` contains `[[models]] id = "opus"`, you've taken
ownership of that id. Future releases that update `opus` (e.g. bump
`display_name` to "Opus 4.8") **will not reach you** for that entry — your
string wins. The rest of the list keeps updating normally.

Built-ins shipped with **new ids** (e.g. a future `opus-5`) always flow
through — you didn't claim those ids, so the merge doesn't block them.

If you want a built-in entry to start updating again:
- Remove your `[[models]] id = "opus"` block. Built-in opus is back.
- Or copy the fresh entry from `claude.example.toml` and paste it over
  your version. That keeps your other override entries intact.

The `.example.toml` file always shows what the current build ships, so
you can diff your `.toml` against it any time to see what you're
shadowing.

### Rollback to defaults

Three levels of undo, smallest to biggest:

| Action | Effect |
|--------|--------|
| Delete one `[[models]]` entry from `claude.toml` | That model falls back to built-in (if id matches a built-in) or disappears (if it was your custom id). |
| Delete `claude.toml` entirely | All built-ins restored; auto-updates work normally. |
| Delete the whole `~/.codeterm/models/` folder | Same as above for every provider, AND `README.md` + `<provider>.example.toml` files get re-installed on next CodeTerm start. |

The `.example.toml` files (`claude.example.toml` etc.) and `README.md` are
**never** read by CodeTerm and are **always** safe to delete — they'll
regenerate. Only `<provider>.toml` is load-bearing.

### Lookup precedence

1. If `<data_dir>/models/<id>.toml` exists and is parseable:
   - `replace = true` → use the file's `[[models]]` verbatim.
   - `replace = false` (default) → merge built-ins with the file by id.
2. No override file → registered built-ins straight from the plugin.

Syntax error or no `[[models]]` / `[[efforts]]` entries? CodeTerm logs a warning
and falls back to plugin built-ins — the picker never goes empty.

## Override format

CodeTerm auto-installs the following on startup:

```
~/.codeterm/models/
  README.md                   ← short pointer (one-time, on first run)
  claude.example.toml         ← always-current built-ins (rewritten every startup)
  codex.example.toml
  opencode.example.toml
```

`.example` files exist for **discovery** — they show what ships with the
current build. They are rewritten on every CodeTerm start and never
participate in lookups.

To enable an override, copy and edit:

```bash
cd ~/.codeterm/models/
cp claude.example.toml claude.toml
# edit claude.toml — CodeTerm picks it up on next read (no restart)
```

Delete `claude.toml` (the override) to fall back to built-ins.

Inside the file:

```toml
# Optional: which model is selected by default for fresh sessions.
default = "opus"

# Optional: which reasoning effort is selected by default.
default_effort = "medium"

# Optional: when true, IGNORE built-ins and use only the list below.
# When false / absent (default), the list below MERGES with built-ins by id.
# replace = true

[[models]]
id = "opus"
display_name = "Claude Opus 4.7 (1M context)"
description = "Most capable for complex reasoning."

[[models]]
id = "sonnet"
display_name = "Claude Sonnet 4.6"
description = "Best for everyday tasks."

[[models]]
id = "haiku"
display_name = "Claude Haiku 4.5"
description = "Fastest for quick answers."

[[efforts]]
id = "xhigh"
display_name = "Extra high"
description = "Maximum reasoning depth."
```

| Field          | Required | Notes |
|----------------|----------|-------|
| `default`      | no       | One of the `id`s. Falls through to plugin default if absent. |
| `id`           | yes      | Must match what the CLI accepts (`claude --model <id>`, `codex -m <id>`). Wrong id → launch failure. |
| `display_name` | yes      | Shown in pickers. Free text. |
| `description`  | no       | Secondary description text. |

`[[efforts]]` entries use the same required `id` and `display_name` fields and
optional `description`. They merge by id like models, so adding `xhigh` keeps
the provider's shipped levels and makes `xhigh` valid for capability-based
launch validation. With `replace = true`, both lists are replaced by the file.

Unknown top-level fields or sections are parse errors: CodeTerm logs the error
and keeps provider defaults instead of silently discarding user configuration.

Empty file or syntax error → CodeTerm logs a warning and falls back to built-ins.
Picker never goes empty.

## Provider Lists

Source of truth:

| Provider | Source |
|----------|-------------|
| `claude` | `src-tauri/src/plugin/agent_claude/provider/schemas.rs` → `builtin_models()` fallback. Claude Code currently exposes model aliases in the TUI/help text, but no stable machine-readable catalog command. |
| `codex`  | `codex debug models` at runtime, cached for 5 minutes; fallback is `src-tauri/src/plugin/agent_codex/models.rs` → `fallback_models()`. |
| `opencode` | `opencode models` at runtime, cached for 5 minutes; fallback is `src-tauri/src/plugin/agent_opencode/models.rs` → `fallback_models()`. |

If upstream ships a new model and the provider has no stable live catalog path
yet, drop a `<provider>.toml` here to update without waiting.

## Adding a new agent provider plugin

If you're writing a new agent plugin and want it to participate in the
catalog (recommended for any plugin with multiple models):

1. **Declare your built-in list as pure data**:
   ```rust
   pub(crate) fn builtin_models() -> Vec<ModelInfo> {
       vec![
           ModelInfo { id: "...".into(), display_name: "...".into(), description: Some("...".into()) },
           // ...
       ]
   }
   ```

2. **Register it in your `new()` constructor**:
   ```rust
   impl MyProvider {
       pub fn new() -> Self {
           crate::agent::model_catalog::register(
               "my-provider",
               crate::agent::model_catalog::RegisteredDefaults {
                   models: builtin_models(),
                   default: Some("my-default-id".into()),
               },
           );
           Self
       }
   }
   ```

3. **Delegate trait methods to the catalog**:
   ```rust
   fn available_models(&self) -> Vec<ModelInfo> {
       crate::agent::model_catalog::resolve_models("my-provider")
   }
   fn default_model(&self) -> Option<String> {
       crate::agent::model_catalog::resolve_default("my-provider")
   }
   ```

4. **(Optional) Plug into `settings_schema()`** — the existing
   `provider::model_select_section(...)` helper takes the resolved list and
   default and renders a Settings → Agents → Model dropdown for free.

That's it. Users get a `<data_dir>/models/my-provider.toml` override surface
automatically.

### When the plugin does dynamic model discovery

Codex and OpenCode poll their CLI catalogs at runtime because availability can
depend on the installed CLI version and account. The catalog supports this:

```rust
fn available_models(&self) -> Vec<ModelInfo> {
    // 1. User override wins outright.
    if let Some(o) = crate::agent::model_catalog::try_override("codex") {
        return o;
    }
    // 2. Dynamic CLI fetch.
    if let Some(cached) = cached_codex_models() { return cached; }
    fetch_codex_models().unwrap_or_else(|_| fallback_models())
}
```

`try_override` returns `Some(...)` only when an override file exists; no override
→ `None`, and the plugin keeps its dynamic path.

## Availability is three-state

A model carries `availability`: `available`, `unavailable`, or `unknown`.

- `unavailable` means the provider **asked and was told no** — a CLI below a
  version gate, or an account list that does not include the model. The picker
  greys the row out and `codeterm agent models <provider>` prints
  `(unavailable)`.
- `unknown` means the probe **could not run at all** — the CLI did not resolve,
  exited non-zero, or produced no parsable version. The row stays selectable,
  both surfaces print `(availability unknown)`, and the verdict is retried.
- The legacy `available` boolean is still published for definite verdicts only.
  An unknown verdict publishes no boolean and is never written to
  `<provider>.cache.json` as a negative.

## Cache invalidation

`<provider>.cache.json` records the CLI it was captured against (path, size,
mtime). Two things void it without any timer:

- **Provider-binary change.** A watcher on the *resolved* CLI path forces a
  re-probe when the file is rewritten, and a snapshot whose recorded binary no
  longer matches is not primed at startup.
- **Opening the model picker.** An explicit user action, TTL-gated to at most
  one probe per freshness window — except while a verdict is still `unknown`,
  which is always retried.

Every refresh clears the `host.exec` binary-resolution memo first: a refresh
that re-reads a memoized failed resolution just rewrites the same wrong answer.

## CLI helpers

There aren't dedicated CLI subcommands for the catalog yet — just edit the
files. To list what CodeTerm currently sees:

```bash
codeterm agent providers --json | jq '.providers[] | {id, default_model, models: .available_models | map(.id)}'
```

## See also

- `~/.codeterm/models/README.md` — short user-facing pointer (auto-installed)
- `src-tauri/crates/codeterm-plugin/src/model_catalog.rs` — override/catalog implementation
- `essences/contributing/architecture.md` — plugin architecture overview
