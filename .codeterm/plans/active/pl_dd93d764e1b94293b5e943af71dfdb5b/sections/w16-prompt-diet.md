---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_2614235822b5406091970347ae1c750c
handle: w16-prompt-diet
title: Wave 16 — Prompt diet (progressive disclosure)
---

Wave 16 — prompt diet (two sequenced tasks; behavioral change => eval-gated per repo rules).

A. Help/docs parity first (t20): move prompt-only knowledge into tier-1 homes — clap long_help for send --actions keys, pane backing types, spawn shapes; routing marker table into a docs topic; verify every removal candidate in codeterm_cli.md has a tier-1 answer. Areas: cli/src/args/* (long_help), docs/ topics + docs registry, src-tauri/prompts (no removals yet), cli manifest test.
B. Prompt rewrite + eval gate (t21): rewrite codeterm_cli.md to invariants + capability map (<=1,200 words); dedupe routing to the codeterm_rule.md canonical copy; bullet-tighten planner_suffix; one-line pointers replace mechanically-enforced rules. Gates: evals/cli-agent baseline BEFORE the rewrite, rerun AFTER (no discovery/success regression); prompt-evaluator regression; word-count budget assertions; prompt_cli_commands_exist green.
Sequencing: A strictly before B (nothing leaves the prompt without a tier-1 home). Single rust-capable worker may take both in one lane.
