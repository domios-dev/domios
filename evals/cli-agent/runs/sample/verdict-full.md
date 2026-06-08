# Verdict — full 30-case run (claude, all families)

Driven by hand (parallel spawn → `pane chat` read → model judge). Post-fix dev
(introspection/pane-list/mem-save doc cues compiled into the skill).

## Headline: introspection doc cue is VALIDATED
Before the cue, agents used `curl`/`--version`/source-grep. After rebuilding the
skill with the cue, the same prompts now use the real commands:
- `identify` → `codeterm identify` (was `codeterm --version` → wrong 0.1.0)
- `capabilities` → `codeterm capabilities` (was curl + source grep)
- `docs` → `codeterm docs MEMORY` (was reading repo files)

## Scoreboard (30 cases, claude)
**Clean PASS (~22):** identify, capabilities, docs, config.get, cron.list,
keepawake.cycle, mem.{get,save,search}, mesh.{peers,status}, pane.exec,
paste.roundtrip, theme.list, web.link, workspace.create, agent.models, and the
four describe-only cases (agent.spawn, bridge.send, config.set, mesh.connect) all
correctly *described without executing*.

**PARTIAL (right answer, wrong/again-bypassed command):**
- pane.current — used `$CODETERM_PANE_ID` + `pane list`, not `pane current`
- pane.search — used `pane list`, not `pane search`
- workspace.list — reached for the repo's `evals/_lib/dc` wrapper (repo-context bleed)
- pane.list — still appended `agent hierarchy` despite the "no such command" cue

**FAIL / flaky:**
- usage.projection — reached for external `ccusage` instead of `codeterm usage projection` → added cue: usage questions use `codeterm usage`, not ccusage
- usage.latest — flailed (`ls`, `claude --help`) under load
- cron.create.describe — no response (count=0)

## Caveat (methodology)
This run spawned all 30 agents at once. High concurrency degraded several
agents (usage flailing, cron no-response, sessions port-scanning, count=60).
`run-suite.sh` runs cases sequentially (only providers parallel) and avoids this;
manual driving should cap at ~8 concurrent. Treat the FAIL/flaky rows above as
"needs a clean low-concurrency re-run to confirm" rather than firm verdicts.

## Fixes shipped from these findings
- introspection block (identify/capabilities/docs) in codeterm_cli.md — validated
- "pane list IS the hierarchy; no `agent list`/`agent hierarchy`" cue
- "remember/save → `mem save`" cue (helped claude; DeepSeek limit unchanged)
- usage: "don't use ccusage" cue
- prompt doctor hardened to accept `codeterm --global-flag`
