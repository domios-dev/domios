---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_4832dcf263624a52ace162c00ebbfd94
handle: codex-usage-fix-parsing-against-real-status-dual-limit-format-timezone
title: 'Codex usage: fix parsing against real /status (dual-limit format, timezone)'
status: done
---

## What

The Codex usage bubble shows values unrelated to Codex's own truth. Side-by-side evidence (user screenshot, same moment):

| Metric | Codex /status (truth) | CodeTerm widget |
|---|---|---|
| 5h/session | 56% left, resets 16:03 | 60%, resets 19:00 (+~3h) |
| Weekly | 82% left, resets 20:27 Jul 8 | 56%, resets Jul 9 17:18 |

Plus a stray 69% marker over the weekly bar and "last updated 22m ago" staleness.

## Root-cause leads (verify, do not assume)

1. **Upstream format change.** Codex v0.142 /status now prints TWO limit families: the general "5h limit / Weekly limit" block AND a per-model "GPT-5.3-Codex-Spark limit" block with its own 5h/weekly rows. A parser written for the single-block format will misalign rows (weekly 56% smells like a mismatched bar) — locate the codex usage parser (JS provider plugin / usage probe path; poller in src-tauri/src/usage/poller.rs, projection in projection.rs) and check its regex/row assumptions against the new output.
2. **Left-vs-used semantics.** Codex reports "% left"; verify the widget's session_pct/weekly_pct semantic and its label — mixed semantics would misread even with correct parsing.
3. **Timezone.** Reset times shifted ~+3h (user is UTC+3): parser likely treats a UTC timestamp as local (or vice versa) when computing "resets HH:MM / in Nh". Find the conversion and pin it.
4. **Staleness.** 22m-old data on an open bubble — check refresh_now wiring for codex panes and the poller cadence/backoff for this provider.

## Fix requirements

- Parser handles BOTH format generations (pre/post dual-limit) keyed by structure, not version string; picks the GENERAL limit family for the main bars and ignores (or separately surfaces) per-model families.
- Fixture-locked tests: capture the real v0.142.4 /status output (user's screenshot text is a starting fixture) + one legacy fixture; parser tests assert exact percentages and reset instants (as UTC epochs) from both.
- Timezone conversion tested across a non-UTC local zone.
- Semantics ("% left" vs "% used") asserted in one shared place used by widget and table.
- Parity discipline: if the parser lives in the JS drop-in, parity_test coverage per the plugin boundary rules.

## Testing gap (user's meta-point)

This feature shipped without provider-output fixtures — that is why an upstream format change silently corrupted values. The fixture suite IS the deliverable as much as the fix.
