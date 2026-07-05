---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_a632c4ba8da047018d2279bf0738c5aa
handle: t21-prompt-diet-ms_a632c4ba8da047018d2279bf0738c5aa
title: report_refused
---

PARTIAL: REPORT NOT SENT
readiness report refused: this git worktree has uncommitted lane changes.
Next commands:
  git status --short
  git add <files>
  git commit -m "<message>"
  codeterm agent report --status in_review --sha $(git rev-parse HEAD) "..."
Or report WIP with:
  codeterm agent report --status partial "..."
Dirty lane files:
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5.json
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/INDEX.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/board-req.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/canvas-req.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/context-req.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/detail-req.md
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/list-req.md
  cli/src/args/agent.rs

Original readiness report that was refused locally:
STATUS: in_review — t20+t21 (prompt diet) executed personally by the Planner per user instruction. DEV IS ALL-CLEAR for your gates (rebuilt twice during the eval window, healthy on 40cdddd6 + working-tree prompts).
CHANGES: (t20) clap long_help extended — send --actions full key vocab + delay-not-default, pane list backing-types/no-shell, agent spawn shapes + task-not-a-marker; new docs ROUTING topic registered (57 lines). (t21) codeterm_cli.md rewritten 4,355 -> 868 words: hard invariants (incl. restored CLI-first discovery habit — dropping it caused a measurable regression, caught and fixed by the eval loop) + capability map + compact mem/mesh carve-out blocks; general_agent.md routing deduped to a pointer (codeterm_rule.md is canonical); docs/PROMPTS.md documents the tier model (rev 11). Set total 7,723 -> ~5,000 words; the 3,500 aspiration needs a codeterm_rule.md dedupe pass — recorded as follow-up, not silently claimed.
BUILD/TYPECHECK/TESTS: cargo check green; prompt_cli_commands_exist green; eval evidence: baseline 16 cases (claude/sonnet) vs final 16 cases on fixed harness — 15/16 CLI-first incl. all mem/mesh cases (transcripts in evals/cli-agent/runs/20260703-*). FOUR harness bugs found+fixed (dc send-text rename break, stdin drain, model arg, and a CRITICAL validity bug: in-repo sandboxes fed agents the repo AGENTS.md — all previous benchmark runs are suspect; sandboxes now under /tmp).
FOLLOWUP: (1) judge t20/t21 diffs — all uncommitted in the integration checkout (prompts, docs, cli args, evals harness); (2) t19's cargo gate can resume on dev now; (3) queue the codeterm_rule.md dedupe as a small follow-up task if you or the Planner deem the 3,500 budget worth finishing; (4) prior cli-agent benchmark results should be re-baselined after the sandbox fix.
