# Provider Plugin Migration — kill the native/JS duality (#45)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans.
> Each phase ends with `cargo test --lib <scope>` green before the next.

**Goal:** one provider implementation path — `js_runtime/provider.rs` (wrapper +
manifest generics) + `plugins/provider-*/plugin.js`. Delete
`src-tauri/src/plugin/agent_{claude,codex,opencode}`, parity tests, and the
`CODETERM_NATIVE_PROVIDERS` rollback flag.

**Why:** the duality already produced three double-fixes in one QA day
(session theft, TUI fingerprints, verbatim cwd) and one silent regression
(usage tracking dead for all providers — poller filters on
`usage_parser().is_some()`, wrapper returns the trait default `None`).

## Current gap inventory (verified by code reading, 2026-06-10)

| Surface | Native impl | Wrapper today | Migration |
|---|---|---|---|
| `usage_parser()` / `usage_probe_command()` | claude `usage.rs`, codex `status_tui.rs` (`CodexStatusUsageParser`) | trait default `None` → poller skips ALL providers | plugin.js `parseUsage(rawText)` + manifest `usageProbeCommand`; wrapper exposes a VM-backed `UsageParser` adapter |
| `read_usage(cwd, sid)` | claude reads JSONL tail | trait default `None` | port to plugin.js `readUsage(cwd, sid)` if any caller besides the poller exists (verify callers first) |
| `detect_session_model` | claude tiered `scan_tail` 64K→512K→4MB | `try_call("detectSessionModel")`, **no native fallback** | verify plugin.js parity with the tiered scan; port the tiers |
| hooks (`has_hooks`) | claude `integration.rs` (install) + `hook_routes.rs` hardcodes `"claude"` | `manifest.spawn.has_hooks` (claude plugin.json already true) | make `ensure_agent_state` provider-generic; move install into wrapper `install_integration` driven by manifest |
| chat fallbacks (`parse_session_messages`, `session_jsonl_path`, `uses_structured_chat`, `read_structured_chat`) | all three | wrapper delegates to `native()` when plugin lacks the export | implement exports in plugin.js; opencode needs a **host sqlite API** (`host.sqlite.query(path, sql, params)` readonly) |
| `detect_agent_events` / `detect_prompt` / `prompt_response_sequence` | detector.rs machinery | wrapper has methods (verify coverage per provider) | verify + port stragglers |
| `screen_has_tui` etc. | both (parity-tested) | plugin-first | already dual — delete native side at flip |

## External native references to clean at flip

- `lib.rs:331-355` native plugin activation; `lib.rs:388-442` collision logic + env flag
- unit-test fixtures: `service/orchestrator.rs`, `service/agent_restart.rs`,
  `agent/registry.rs`, `update/shutdown.rs` — replace `ClaudeProvider::new()`
  registrations with a local `TestProvider` stub (or load the JS plugin like
  `parity_test.rs` does)
- `bin/parser_eval.rs` — switch to JS plugin parsers or retire the binary
- `plugin/mod.rs:12-14` module decls

## Phases

1. **Usage**: host API for probe-text parse; plugin.js `parseUsage` ports the
   regexes from `agent_claude/usage.rs` + codex `status_tui.rs` fixtures as JS
   tests; wrapper `usage_parser()` returns VM adapter; `usage_probe_command`
   from manifest. Gate: usage poller picks up all three providers on dev;
   `cargo test --lib usage` green.
2. **Model detect**: port tiered scan to plugin.js (host needs `readFileTail`);
   parity-check against `model_label_tests` fixtures, then move those fixtures
   to plugin tests.
3. **Hooks**: `hook_routes::ensure_agent_state` drops the hardcoded "claude"
   (resolve provider by pane agent_type; default via manifest `hasHooks`);
   integration install goes through the wrapper.
4. **Chat fallbacks**: plugin.js exports for claude/codex; `host.sqlite`
   (rusqlite, readonly open) for opencode structured chat; delete `native()`
   delegation.
5. **Flip**: stop activating natives in lib.rs; rewrite test fixtures; delete
   `agent_{claude,codex,opencode}` modules, parity_test.rs, env flag,
   `registry.native_fallback`. Full `cargo test --lib` + Playwright 03/05/12.

## Status log
- [x] **Phase 1 usage** — `parseUsage(rawText, nowMs)` ported to claude+codex
  plugin.js (regex + reset-clock parsing), returns the snake_case UsageSnapshot
  shape; `usageProbeCommand` added to both manifests + SpawnConfig; wrapper
  exposes `VmUsageParser` via `usage_parser()` and `usage_probe_command()` from
  manifest. Node-verified both against the native fixtures (claude 43/23%, codex
  10/80%, empty→null, garbage→null, partial). JS regex gotcha: strings feeding
  `new RegExp` need DOUBLE backslashes (single collapses in the string literal).
- [ ] Phase 2 model detect
- [ ] Phase 3 hooks
- [ ] Phase 4 chat fallbacks + host.sqlite
- [ ] Phase 5 flip + delete

> Detour during this work (not migration, but found via web QA): the web chat
> composer's `write_pty_string` reached the right pty but raced claude's TUI
> boot — bracket-paste swallowed, no reply. Fixed by routing web `raw:false`
> writes through `send_text_to_pty_smart` (verified injection) with
> `require_prompt_first` + boot-wait. Tracked under #44.
