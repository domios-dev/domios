---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_1cc6b5e29b904fe8aabac857a6069687
handle: prompts-cross-machine-orchestration-etiquette-ga-redirects-orchestrators-ms_1cc6b5e29b904fe8aabac857a6069687
title: decision
---

TEXT D + CAP AMENDMENT (round-55). Amending my own 'third and LAST insertion' pin: TEXT D lands in orchestration_toolset.md (a DIFFERENT file — the suffix <=12-line budget stands untouched; mechanics belong in the toolset, not role identity). OVERLAP CHECK against existing work (user asked): the no-poll rule EXISTS but only for REPORTS (orchestrator_suffix.md line 2: 'Do NOT use sleep or poll for reports... auto-injected') — shell gates are uncovered, which is exactly the hole Puccini fell into; wake-on-report (done) fixed injected-delivery submit, irrelevant to shell panes; reflection tripwire (done) covers integration-checkout edits only. So TEXT D is NEW scope, not duplication. INSERT VERBATIM into orchestration_toolset.md: 'Long-running shell gates (builds, soaks, remote regressions): never wait by polling — sleep/get-text loops are implementer-mode waiting. Give the command a completion contract at launch: <cmd>; codeterm send --pane 636d967b [--mesh <your-peer>] "GATE:<task_id> rc=0" — then record the gate on the task and work other frontier items until the marker arrives. For an already-running command, start a companion watcher in that pane (while pgrep -f <name> >/dev/null; do sleep 30; done; codeterm send ...): the wait runs remotely, never in your loop.' Wiring-test key phrase: 'completion contract at launch'.
