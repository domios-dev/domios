---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_29dda747f62249ababd8306ba8a9e27b
handle: t21-prompt-diet
title: Prompt diet rewrite + cli-agent eval gate
status: done
---

Prompt rewrite to the three-tier model + eval gate (spec section prompt-diet; wave w16 part B). STRICTLY after t20 (use its chunk->home mapping).

Steps:
1. BASELINE FIRST: run evals/cli-agent against the current prompts; record scores (repo rule: no behavioural change without a bench baseline).
2. Rewrite src-tauri/prompts/codeterm_cli.md to: env vars + hard invariants (routing pointer, don't-echo, chat_backend-no-shell, delayed-not-default, report contract, no hardcoded models) + capability map (domain -> trigger -> `--help`/docs entry point). Budget <=1,200 words.
3. Dedupe: codeterm_rule.md keeps THE routing table; general_agent.md routing section becomes a 2-line pointer; planner_suffix.md -> imperative bullets; mechanically-enforced rules (heartbeat contract once t19 lands, planner-lane hint) -> one-line pointers.
4. Word-count assertions in a test or check script (set <=3,500 total, cli <=1,200); prompt_cli_commands_exist green over the remaining inline commands.
5. Rerun evals/cli-agent AFTER; compare to baseline — command discovery/success must stay within noise; run prompt-evaluator for general_agent/worker/orchestrator prompts.
6. Docs: docs/PROMPTS.md describes the tier model; revision bump.
Report: before/after word counts, eval score table baseline vs after, chunk->home mapping applied, any content that had to stay inline and why.
Commands: bash evals/cli-agent per its README (baseline + after); ./build.sh check; ./build.sh cli; prompt-evaluator run.

CARVE-OUT (user requirement, added): mem and mesh each KEEP a compact inline tier-0 block (~150-250 words: behavioral protocol + core commands; depth defers to docs MEMORY/MESH). CLI-section budget therefore <=1,800 words (set still <=3,500). Do not reduce mem/mesh to bare capability-map lines.
