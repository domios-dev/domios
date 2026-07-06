---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_703fc24a931e4925b612443b238ee6d3
handle: polling-smell-detector-nudge-orchestrators-stuck-in-get-text-loops-ms_703fc24a931e4925b612443b238ee6d3
title: note
---

SCOPE ADDITION from the orchestrator token audit (round-92): yield-watching is polling too. The audit found the orchestrator holding write_stdin yields (yield_time_ms:30000, five+ consecutive cycles with progress narration between: '700/912 crates... 880/912...') on a cold build — same violation class as get-text loops, different API shape. The detector must count write_stdin calls with yield_time_ms above ~5s against the same threshold as pane get-text/status polls.
