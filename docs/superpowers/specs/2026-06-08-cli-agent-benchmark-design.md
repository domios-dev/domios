# CLI Agent Benchmark — Integration-Test LLM Benchmark for the CodeTerm CLI

<!-- revision: 1 -->

Branch: `feat/system-plugins-intrinsic`.

## Problem

Current `evals/general-agent/` and `evals/mem-protocol/` test prompt behavior via
`claude -p` headless: the agent *describes* what it would do but never executes,
and only Claude is covered. That tells us nothing about whether a real agent in
the dev console understands the CodeTerm CLI, fetches and uses the skill, and
actually runs the right commands — across Claude, Codex, and opencode.

## Goal

A real-daemon integration benchmark that drives **real agents** in the dev
console (port 7686) through the same internal mechanisms users rely on
(`agent spawn` / `send` / structured-chat read), exercises every CLI
command via natural-language asks, and uses **the model (Claude, in this
session) as judge** — reading the parsed chat transcript and scoring how well
each agent understood and executed. Raises CLI reliability by surfacing
doc/skill/CLI bugs, which we fix in the same loop.

Non-goals: an automated rubric scorer (judging is the model's job); replacing
the Rust `mem-recall` bench or the `session-restore`/`dreamer-repair` daemon
evals (those stay).

## Architecture

New directory `evals/cli-agent/`, built on the existing dev-locked access layer
(`evals/_lib/lib.sh`: `dc`, `dev_get`, `dev_post`, `new_*_pane`).

```
evals/cli-agent/
  README.md            — what it is, how to run, safety
  SKILL.md             — the benchmark skill (protocol + rubric + judging guide)
  cases/               — declarative test cases, one file per command family
    workspace.yaml  pane.yaml  agent.yaml  mem.yaml  cron.yaml
    send.yaml  mesh.yaml  plugin.yaml  misc.yaml  ...
  lib.sh               — harness helpers layered on _lib/lib.sh
  run-case.sh          — drive ONE case for ONE provider → transcript
  run-suite.sh         — iterate cases × {claude,codex,opencode} in parallel
  judge-template.md    — the per-run verdict skeleton the model fills in
  runs/<ts>/           — generated transcripts + verdict (gitignored except samples)
```

### Test-case format (`cases/<family>.yaml`)

A case tests one command via a natural-language ask. We test whether the agent
*figures out* the command, not whether it can copy a literal command.

```yaml
- id: mem.search.basic
  command: "mem search"          # CLI command under test (must exist in cli-manifest.json)
  prompt: "Find what we know about the Telegram bridge in memory."
  followups: []                  # optional extra nudges if the agent stalls
  expect: |                      # judge guidance — what good looks like
    Runs `codeterm mem search "<telegram bridge query>"`, reads results,
    summarizes. Should NOT invent a `mem find` or hit the daemon socket wrong.
  safety: execute                # execute | describe-only
  providers: [claude, codex, opencode]   # default: all three
```

- `command` is validated against `docs/cli-manifest.json` by a harness check
  (reusing the SoT manifest) so a case can't target a non-existent command.
- `safety: describe-only` cases ask the agent to *explain or dry-run* rather than
  execute (see Safety). The judge scores understanding, not side effects.

### Coverage

One case file per command family; every command in `docs/cli-manifest.json` gets
at least one case (a harness check reports uncovered commands). Trivially-similar
subcommands (e.g. `cron enable`/`disable`) may share a parametrized case. The
catalog is data, not prose — kept compact.

## Drive → read → judge loop

**Drive** (`run-case.sh <case-id> <provider>`):
1. `dc workspace create "bench-<case>-<provider>" --cwd <throwaway tmp>`.
2. `dc agent spawn <provider> --workspace <ws> --cwd <tmp> --title <case>`.
3. Wait for readiness, `dc send "<prompt>" --pane <pane>`.
4. Poll the new normalized-chat read (below) until the transcript is idle
   (no new assistant/tool messages for N seconds) or a timeout.
5. If `followups`, send each only when the agent stalls without success.
6. Dump transcript → `runs/<ts>/<case>-<provider>.json` (+ readable `.md`).
7. Tear down the workspace.

**Read** — new stateless primitive (Rust): `codeterm pane chat --pane ID
[--json] [--since N]` and `GET /api/panes/{id}/chat?since=N`. Returns the
**already-parsed normalized messages** (the same model the UI renders:
text / thinking / tool_call{name,input} / tool_result), no tailer spawned.
Reuses the existing chat-snapshot parse path; `since` is a message index for
incremental reads. Works uniformly for claude (JSONL) and opencode (sqlite).
This is a new clap command → it auto-appears in `cli-manifest.json` + `docs/CLI.md`
and must pass the prompt doctor (dogfoods the SoT system).

**Judge** — the model reads each transcript and scores five dimensions:

| # | Dimension | PASS means |
|---|-----------|-----------|
| 1 | Understood | grasped the ask, didn't go sideways |
| 2 | Correct command + flags | used the right `codeterm …` invocation |
| 3 | Skill usage | fetched/used the codeterm skill or `docs`, didn't hallucinate flags |
| 4 | Step efficiency | nudges-to-success (0 = one-shot); record the count |
| 5 | Acted, not described | a real tool_call + result is in the transcript |

Each dimension PASS / PARTIAL / FAIL + one-line note. Verdict written to
`runs/<ts>/verdict.md`, aggregated per command and per provider. The judge also
records, per case, **bugs found** (doc gap / card mis-render / CLI bug) as
actionable items.

**Final multi-step session** — one free-form session per provider that chains
several safe operations (create workspace → spawn sub-agent → send → mem save →
search) to assess multi-step reliability and how many nudges a realistic flow
needs.

## Bug-fix loop

When a verdict attributes a failure to:
- **Unclear docs** → fix `src-tauri/prompts/codeterm_cli.md` (the shared piece
  included into both the general-agent prompt and the seed skill; doctor-guarded),
  rebuild, re-run the case.
- **Card mis-render** (e.g. opencode `tool:"bash"` codeterm commands not showing
  the CodeTerm card) → fix `packages/chat-ui`, rebuild web, re-run.
- **Real CLI bug** → fix Rust, rebuild, re-run.

Fixes land in the same branch; the verdict notes the fix commit.

## Safety

The benchmark must never touch prod-affecting or irreversible state. Per-family
classification baked into the cases:

- **execute** (run for real in a throwaway dev workspace): workspace, pane,
  agent (spawn/report/list-ish/model/restart within bench panes), send,
  mem (save/search/get/update/skill — in dev store), cron (create/list/trigger/
  delete — dev, `--once` far-future or immediately-deleted), keepawake (short
  duration then stop), docs, usage, theme (list/dir), config get, capabilities,
  identify, cli-manifest, web link, paste.
- **describe-only** (agent explains / dry-runs, never executes): mesh connect/
  approve/disconnect/exec/send (real peers), bridge send/react/migrate (real
  Telegram), mem secret*, system enable/disable-autostart, update install,
  config set, theme apply/delete, plugin install/remove/update (mutates the dev
  plugin set), mem forget/delete of non-bench items, mem graph destructive ops.

`run-case.sh` refuses to `send` an execute-prompt for a `describe-only`
case. All workspaces are torn down after each case.

## evals/ cleanup

- **Replace**: `evals/general-agent/` and `evals/mem-protocol/` (the `claude -p`
  headless runners). Migrate their still-useful behavioral intent (recon-before-
  action, protocol-call firing) into real-agent cases where it fits; delete the
  headless runner wiring + the `prompt-evaluator` dependency for these two.
- **Keep untouched**: `evals/mem-recall/` (Rust bench), `evals/session-restore/`,
  `evals/dreamer-repair/`.
- **Leave for now**: `evals/dreaming/` (REM-phase prompt eval; out of scope).
- Update `evals/README.md` to describe the new `cli-agent/` harness and the
  two-tier split (Rust/daemon scripted evals vs the real-agent LLM benchmark).

## The benchmark skill (`evals/cli-agent/SKILL.md`)

In-project skill so any agent (or a future session of me) can run the benchmark:
the drive→read→judge protocol, the rubric, how to read transcripts via the new
`pane chat` command, the safety policy, and the doc/skill bug-fix loop. Registered
with mem (`codeterm mem skill add evals/cli-agent`) on the dev instance.

## Components & isolation

| Unit | Does | Depends on |
|------|------|-----------|
| `pane chat` cmd + endpoint | stateless normalized-chat read | existing chat parse |
| `cases/*.yaml` | declarative asks + expectations | cli-manifest (validation) |
| `run-case.sh` | drive one case/provider → transcript | _lib/lib.sh, pane chat |
| `run-suite.sh` | fan out cases × providers | run-case.sh |
| judge (model) | score transcripts, find bugs | transcripts, rubric |
| `SKILL.md` | document the protocol | all of the above |

## Error handling

- Agent never reaches success within timeout + max nudges → case marked FAIL
  (step-efficiency = ∞), transcript kept for the judge.
- Provider unavailable (e.g. codex model picker) → case marked SKIPPED with reason.
- Daemon down → `dc` exits 7 (lib.sh invariant); suite aborts loudly.

## Testing

- The new `pane chat` read: Rust unit test (parses a known JSONL/sqlite fixture
  into normalized messages) + it must pass `cli_manifest_is_fresh` /
  `prompt_cli_commands_exist` after regen.
- The harness: a smoke run of one `execute` case against one provider that asserts
  a non-empty transcript with at least one tool_call.
- The benchmark itself is the test of the CLI surface; its output is verdicts, not
  a green/red CI gate (it's model-judged, run on demand).

## Rollback

Additive: new `evals/cli-agent/`, one new read command, deletion of two headless
eval folders. Revert the commits; the kept evals are untouched.
