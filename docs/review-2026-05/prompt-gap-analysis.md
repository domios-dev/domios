# Prompt ↔ Code Gap Analysis

Audit of every agent-facing prompt against the code that actually serves it.
Catches: commands/flags that don't exist, behavior the prompt claims but the code
doesn't do (or does differently), dead references, cross-prompt duplication,
**undeclared** features (in code, never surfaced to agents), and **lost** features
(prompt still references something removed).

**Method:** every command / flag / behavioral claim checked against CLI arg enums
(`cli/src/args/*.rs`) + handlers (`cli/src/cmd/*.rs`), REST routes (`src-tauri/src/api/`),
service layer (`src-tauri/src/service/`), and config schema (`src-tauri/src/config/`).

**Status: COMPLETE first pass.** All 13 prompts audited. Findings below.

---

## Prompt inventory

| File | Role | Audited | Verdict |
|------|------|:---:|---------|
| `codeterm_cli.md` | Shared CLI reference (GA + skill) | ✅ | G3, G5, G6 |
| `general_agent.md` | General Agent system prompt | ✅ | clean (resolved G2 in its favor) |
| `bootstrap_instructions.md` | First-run onboarding | ✅ | clean |
| `codeterm_rule.md` | Base rule → `~/.claude/rules/` | ✅ | **G1, G2**, G4 |
| `worker_suffix.md` | `--role Worker` suffix | ✅ | clean |
| `orchestrator_suffix.md` | `--role Orchestrator` suffix | ✅ | clean |
| `worker_task_preamble.md` | Mem graph-worker wrapper | ✅ | clean |
| `cron_report_hint.md` | Cron task addendum | ✅ | clean |
| `cron_complete_suffix.md` | Cron task addendum | ✅ | clean |
| `bridge_mem.md` | Mem protocol rule | ✅ | clean |
| `mem_rerank.md` | LLM rerank prompt | ✅ | clean (no CLI claims) |
| `mem_extraction.md` | LLM entity extraction | ✅ | clean (no CLI claims) |
| `dreaming_rem.md` | Dreamer REM prompt | ✅ | clean (graph CLI matches) |

---

## Findings

Severity: **BUG** (wrong, will fail) · **CONTRADICTION** (two sources disagree) ·
**DEAD** (references something nonexistent) · **DUP** (same content in N places, drifts) ·
**UNDECLARED** (code feature never surfaced) · **MINOR/VERIFY**.

| ID | File:line | Type | Finding | Fix |
|----|-----------|------|---------|-----|
| **G1** | `codeterm_rule.md:26` | **BUG** | `codeterm send-text PANE_ID "command"` — wrong arg order. CLI is `SendText { text (positional), --pane }`, so the pane id is parsed as the *text* and the command is dropped. (`codeterm_cli.md:25` and `codeterm_rule.md:39` already use the correct form.) | `codeterm send-text "command" --pane PANE_ID` |
| **G2** | `codeterm_rule.md:74` | **CONTRADICTION / BUG** | "Default backend is Bitwarden (uses the `bw` CLI)." The real default is **`local`** (OS keychain) — `config/schema/mod.rs:520` `default_secrets_backend()` → `"local"`. `general_agent.md:52` says "Local OS keychain by default" (correct). So `codeterm_rule.md` is wrong AND disagrees with the GA prompt. | Reword to: default backend is `local` (OS keychain); Bitwarden/Vaultwarden when configured. |
| **G3** | `codeterm_cli.md` ↔ `docs/API.md` ↔ `docs/SKILLS.md` | **DUP** | CLI command syntax lives in 3 places. This is the drift that left stale `agent hierarchy` / `Lead` refs when commands changed (fixed this session). `docs/contributing/prompts.md:99` already forbids duplicating CLI syntax between prompt and skill. | Make `codeterm_cli.md` the single source; have `docs/SKILLS.md` / `docs/API.md` CLI sections point to it instead of re-listing. |
| **G5** | `codeterm_cli.md:47` | **DEAD** | "use `batch` if you want to fire several at once" — there is **no `codeterm batch` CLI command**. Batch is REST (`POST /api/batch`) + Tauri IPC (`mux_batch`) only. An agent that tries `codeterm batch` fails. | Drop the `batch` mention, or replace with "spawn several siblings in separate `agent spawn` calls". |
| **G6** | `codeterm_cli.md` (absent) | **UNDECLARED** | `codeterm usage {latest,projection,refresh}` (token-usage tracking) is **never mentioned** to agents — they can't self-monitor token spend. Also undeclared: `codeterm identify`, `codeterm capabilities`, `agent providers`, `agent models`. (`system`, `debug` intentionally omitted.) | Add a short `usage` block to `codeterm_cli.md` (most valuable). The rest are optional. |
| G4 | `codeterm_rule.md:61,70` | **VERIFY/MINOR** | Secret-save guidance uses `echo -n "$V" \| codeterm mem secret save …`. `echo -n` is bash; Windows PowerShell `echo` appends a newline and ignores `-n`, so a saved value may carry a trailing newline. Project is cross-platform. | Confirm cross-platform stdin trimming; if affected, give a PowerShell-safe form. |

---

## Verified clean (no gap)

- **`general_agent.md`** — `bridge send`, `send-text --pane --mesh`, `agent restart`, `mem graph relate --valid-from/--valid-to`, `mem save --kind preference --replace`, `pane exec`, `mesh peers`, `agent spawn --workspace --cwd`, `docs {TELEGRAM-SETUP,SECRETS,TEXT-HOOKS,CONFIG}` — all exist and match. Secret-default statement (keychain) is correct.
- **`dreaming_rem.md`** — every `mem graph` verb (related, timeline, hubs, islands, orphans, route, introduce-topic, relate, invalidate, alias, revive-broken, purge-invalidated) matches `MemGraphAction` 1:1; `mem worker report` OK.
- **`worker_suffix` / `orchestrator_suffix` / `cron_report_hint`** → `codeterm agent report "..."` — matches `AgentAction::Report`.
- **`cron_complete_suffix`** → `cron complete --run-id {} --summary` — matches `CronAction::Complete`; `{}` filled by `prompts.rs`.
- **`worker_task_preamble`** → `mem worker progress`/`report` — matches `MemWorkerAction`.
- **`bridge_mem` / `bootstrap_instructions`** — all command names valid post-Lead-removal.
- **`mem_rerank` / `mem_extraction`** — pure model prompts, no CLI surface to drift.

---

## Remediation status

- ✅ **G1** — fixed (`codeterm_rule.md:26`, correct `send-text "text" --pane ID`).
- ✅ **G2** — fixed (`codeterm_rule.md:74` now states `local` keychain default, matching code + GA prompt).
- ✅ **G5** — fixed (`codeterm_cli.md:47` dead `batch` reference removed).
- ✅ **G6** — fixed (`codeterm_cli.md` now has a `usage` block; closes the "agents can't see token spend" gap).
- ☐ **G3** — open: structural CLI-ref triplication (`codeterm_cli.md` ↔ `API.md` ↔ `SKILLS.md`). The single-source move; bigger, deferred.
- ☐ **G4** — open: verify the cross-platform (`echo -n` / PowerShell) secret-save form.

> The G1/G2/G5/G6 prompt edits are `include_str!`'d into the daemon — they go live on the next Rust rebuild (`./build.sh dev`).

<!-- revision: 3 -->
