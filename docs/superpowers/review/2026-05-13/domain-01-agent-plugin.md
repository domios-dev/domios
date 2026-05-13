# Domain Review: Agent & Plugin

**Date:** 2026-05-13
**Scope:** `src-tauri/src/agent/` and `src-tauri/src/plugin/` (all `.rs` files)
**Reviewer:** subagent (domain-01)

---

## Files Reviewed

| File | Lines |
|------|-------|
| `src-tauri/src/agent/mod.rs` | 13 |
| `src-tauri/src/agent/detector.rs` | 646 |
| `src-tauri/src/agent/git_info.rs` | 25 |
| `src-tauri/src/agent/model_catalog.rs` | 556 |
| `src-tauri/src/agent/models.rs` | 52 |
| `src-tauri/src/agent/prompt.rs` | 473 |
| `src-tauri/src/agent/provider.rs` | 903 |
| `src-tauri/src/agent/registry.rs` | 554 |
| `src-tauri/src/agent/reports.rs` | 173 |
| `src-tauri/src/agent/run_watcher.rs` | 558 |
| `src-tauri/src/agent/status.rs` | 227 |
| `src-tauri/src/agent/usage_parser.rs` | 54 |
| `src-tauri/src/plugin/mod.rs` | 639 |
| `src-tauri/src/plugin/common.rs` | 12 |
| `src-tauri/src/plugin/agent_claude/mod.rs` | 132 |
| `src-tauri/src/plugin/agent_claude/provider/mod.rs` | 376 |
| `src-tauri/src/plugin/agent_claude/provider/detection.rs` | 209 |
| `src-tauri/src/plugin/agent_claude/provider/launch.rs` | 275 |
| `src-tauri/src/plugin/agent_claude/provider/schemas.rs` | 236 |
| `src-tauri/src/plugin/agent_claude/provider/sessions.rs` | 270 |
| `src-tauri/src/plugin/agent_claude/provider/usage.rs` | 181 |
| `src-tauri/src/plugin/agent_claude/usage_tui/mod.rs` | 64 |
| `src-tauri/src/plugin/agent_claude/usage_tui/resets.rs` | 99 |
| `src-tauri/src/plugin/agent_claude/usage_tui/session_block.rs` | 37 |
| `src-tauri/src/plugin/agent_claude/usage_tui/tests.rs` | 64 |
| `src-tauri/src/plugin/agent_codex/mod.rs` | 130 |
| `src-tauri/src/plugin/agent_codex/provider.rs` | 627 |
| `src-tauri/src/plugin/agent_codex/provider_tests.rs` | 317 |
| `src-tauri/src/plugin/agent_codex/status_tui.rs` | 157 |
| `src-tauri/src/plugin/agent_codex/usage.rs` | 201 |
| `src-tauri/src/plugin/agent_opencode/mod.rs` | 128 |
| `src-tauri/src/plugin/agent_opencode/provider.rs` | 764 |
| `src-tauri/src/plugin/agent_opencode/models.rs` | 138 |
| `src-tauri/src/plugin/agent_opencode/provider_tests.rs` | 365 |
| `src-tauri/src/plugin/agent_opencode/usage.rs` | 206 |

**Total: 35 files**

---

## Findings

### src-tauri/src/agent/mod.rs

#### LOW
- **Line 10-11** — `#[allow(dead_code)]` on `pub mod status`. The module is public but the lint suppression signals no external consumer exists. Either expose it properly via the crate's public API or make it `pub(crate)` and remove the allow.

---

### src-tauri/src/agent/detector.rs

No findings. All production code uses `.ok()?` or match arms; no bare unwrap outside `#[cfg(test)]` blocks. Structural fingerprinting is consistent throughout.

---

### src-tauri/src/agent/git_info.rs

No findings.

---

### src-tauri/src/agent/model_catalog.rs

#### LOW
- **Line 197** — `None.into_iter().next().unwrap_or_else(|| String::new())` is a convoluted equivalent of `String::new()`. The `None.into_iter().next()` always yields `None`; the `unwrap_or_else` always fires. Replace with just `String::new()`.

---

### src-tauri/src/agent/models.rs

No findings.

---

### src-tauri/src/agent/prompt.rs

#### LOW
- **Line 16** — `#[allow(dead_code)]` on the `detected_at` field of `PanePromptState::Active`. If the field was removed it is dead code; if it is intentional for future use, document why.
- **Line 175** — `#[allow(dead_code)]` on public method `active_prompt()`. Dead public API should be removed or promoted to actual use.

---

### src-tauri/src/agent/provider.rs

#### MEDIUM
- **Lines 1-903** — File is at 903 lines, 97 lines from the CI-enforced 1000-line hard limit. The file combines three distinct concerns: all settings schema types (`SettingField` enum, `ModelInfo`, `ProviderDescriptor`, `BadgeVariant`, `SettingFieldType`, etc.), the `AgentProvider` trait with ~30 methods and their defaults, and builder-helper functions (`session_resume_section`, `usage_tracking_section`, `model_select_section`). Any non-trivial addition will breach the limit. Recommended split: move the schema types into `src-tauri/src/agent/schema.rs`, leave the trait + builders in `provider.rs`.

---

### src-tauri/src/agent/registry.rs

No findings. `RwLock` is always acquired short-lived (expression-scoped); no lock held across `.await`.

---

### src-tauri/src/agent/reports.rs

No findings.

---

### src-tauri/src/agent/run_watcher.rs

No findings. Confirmed no lock held across `.await`: all lock acquisitions in the `watchdog_loop` are expression-scoped and released before the `tick.tick().await` yield point. No lock-ordering violations (only `output_buf` from the hierarchy is touched, never nested).

---

### src-tauri/src/agent/status.rs

No findings.

---

### src-tauri/src/agent/usage_parser.rs

No findings.

---

### src-tauri/src/plugin/mod.rs

#### MEDIUM
- **Lines 1-639** — `mod.rs` advisory limit is 300 lines; file is 639 lines (2x over). The file contains the full `PluginManager` implementation (~200 lines of logic), all plugin type definitions (`PluginManifest`, `PluginMeta`, `PluginCapabilities`, `PluginPermissions`, `PluginActivation`, `PluginPrompt`, `PluginTier`, `SkillFile`, `PluginState`, `PluginEntry`), and the `Plugin` trait. This violates the `mod.rs`-as-facade rule. Recommended split: move the type hierarchy to `plugin/types.rs`, `PluginManager` implementation to `plugin/manager.rs`, leave only re-exports and `Plugin` trait in `mod.rs`.

---

### src-tauri/src/plugin/common.rs

No findings. Good single-function module.

---

### src-tauri/src/plugin/agent_claude/mod.rs

No findings.

---

### src-tauri/src/plugin/agent_claude/provider/mod.rs

No findings. Clean facade delegating to sibling modules.

---

### src-tauri/src/plugin/agent_claude/provider/detection.rs

#### MEDIUM
- **Lines 18-28** — `detect_from_output` uses hardcoded literal strings: `"Do you want to proceed?"`, `"may I proceed"`, `"Cogitated for"`, `"Claude Code"`, etc. The architecture rule (CLAUDE.md) states: *"Detection uses structural fingerprints, never literal strings — vendors change copy without notice."* Provider-output detection should use structural signals (output layout, token shape, ANSI escape sequence patterns) rather than verbatim phrases that can silently break on upstream copy changes.

---

### src-tauri/src/plugin/agent_claude/provider/launch.rs

#### MEDIUM
- **Line 14** — `params.task.as_ref().unwrap()` is reached only inside `if params.headless && params.task.is_some()`. The guard makes the unwrap safe today, but the safety invariant is enforced by a parent `if` branch rather than the type system. If the surrounding logic is ever refactored the unwrap becomes a runtime panic. Use `if let Some(task) = &params.task` (or `params.task.as_deref().unwrap_or_default()`) to make the extraction and the guard the same expression. Same pattern exists in Codex and OpenCode providers (see below).

---

### src-tauri/src/plugin/agent_claude/provider/schemas.rs

No findings.

---

### src-tauri/src/plugin/agent_claude/provider/sessions.rs

No findings.

---

### src-tauri/src/plugin/agent_claude/provider/usage.rs

No findings.

---

### src-tauri/src/plugin/agent_claude/usage_tui/mod.rs

No findings.

---

### src-tauri/src/plugin/agent_claude/usage_tui/resets.rs

#### MEDIUM
- **Lines 59-75** — `month_from_name(name: &str) -> Option<u32>` maps full month name strings to month numbers. Identical logic exists in `plugin/agent_codex/status_tui.rs:112-128` as `month_from_short`. Both do the same job (string → u32 month) with slightly different input format. Consolidate into a single function in `plugin/common.rs` that accepts both full and abbreviated names, or two thin wrappers over a shared table.

---

### src-tauri/src/plugin/agent_claude/usage_tui/session_block.rs

No findings.

---

### src-tauri/src/plugin/agent_claude/usage_tui/tests.rs

Test code only — no production findings.

---

### src-tauri/src/plugin/agent_codex/mod.rs

No findings.

---

### src-tauri/src/plugin/agent_codex/provider.rs

#### MEDIUM
- **Line 99** — `params.task.as_ref().unwrap()` inside `if params.headless && params.task.is_some()`. Same fragile guard-then-unwrap pattern as Claude launch.rs. Fix identically.

#### LOW
- **Lines 566-583** — `find_file_by_suffix` performs a recursive walk looking for a file matching a suffix predicate. Functionally identical to `find_file_by_name` in `plugin/agent_opencode/provider.rs:740-759`. Both are private helpers doing the same recursive-walk-by-predicate logic. Extract a shared `find_file_by<F: Fn(&Path) -> bool>` in `plugin/common.rs` and thin-wrap in each provider.

---

### src-tauri/src/plugin/agent_codex/provider_tests.rs

Test code only — no production findings.

---

### src-tauri/src/plugin/agent_codex/status_tui.rs

#### MEDIUM
- **Lines 112-128** — `month_from_short` is a DRY duplicate of `month_from_name` in `plugin/agent_claude/usage_tui/resets.rs:59-75`. See finding on `resets.rs` above.

---

### src-tauri/src/plugin/agent_codex/usage.rs

No findings.

---

### src-tauri/src/plugin/agent_opencode/mod.rs

No findings.

---

### src-tauri/src/plugin/agent_opencode/provider.rs

#### MEDIUM
- **Line 198** — `params.task.as_ref().unwrap()` inside `if params.headless && params.task.is_some()`. Same fragile guard-then-unwrap pattern. Fix identically.

#### LOW
- **Lines 740-759** — `find_file_by_name` — see Codex `find_file_by_suffix` finding above. Shared logic, should be extracted.

---

### src-tauri/src/plugin/agent_opencode/models.rs

No findings.

---

### src-tauri/src/plugin/agent_opencode/provider_tests.rs

Test code only — no production findings.

---

### src-tauri/src/plugin/agent_opencode/usage.rs

No findings.

---

## Summary Table

| Severity | Count |
|----------|-------|
| CRITICAL | 0 |
| HIGH | 0 |
| MEDIUM | 7 |
| LOW | 5 |
| **Total** | **12** |

---

## Grouped Action Items

### Immediate (before next 1000-line breach)

1. **Split `agent/provider.rs`** — extract schema types to `agent/schema.rs`. Saves ~300 lines, brings the file well under the limit.

### Short-term (cleanup sprint)

2. **Refactor `plugin/mod.rs`** — move `PluginManager` impl to `plugin/manager.rs`, types to `plugin/types.rs`, keep `mod.rs` as a facade.
3. **Fix three guarded unwraps** — `claude/provider/launch.rs:14`, `codex/provider.rs:99`, `opencode/provider.rs:198`. Replace `if is_some() { .unwrap() }` with `if let Some(task) = &params.task`.
4. **Consolidate month-name parsers** — merge `month_from_name` and `month_from_short` into `plugin/common.rs`.
5. **Extract shared recursive file walk** — `find_file_by_suffix` / `find_file_by_name` → `plugin/common.rs`.

### Architectural (design discussion)

6. **`detection.rs` literal strings** — `ClaudeProvider::detect_from_output` uses verbatim phrases. Consider migrating to structural signals aligned with the architecture rule, or explicitly document the exemption and add a test fixture for each literal so breakage is caught on update.

### Trivial (any PR)

7. **`agent/mod.rs`** — remove or address `#[allow(dead_code)]` on `pub mod status`.
8. **`agent/prompt.rs`** — remove dead `detected_at` field and `active_prompt()` method, or promote to actual use.
9. **`agent/model_catalog.rs:197`** — replace `None.into_iter().next().unwrap_or_else(|| String::new())` with `String::new()`.
