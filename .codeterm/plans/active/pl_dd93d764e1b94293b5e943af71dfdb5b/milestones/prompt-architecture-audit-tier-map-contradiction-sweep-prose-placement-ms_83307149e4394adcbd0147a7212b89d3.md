---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_83307149e4394adcbd0147a7212b89d3
handle: prompt-architecture-audit-tier-map-contradiction-sweep-prose-placement-ms_83307149e4394adcbd0147a7212b89d3
title: decision
---

Critical prompt-architecture contradictions fixed directly on feat/canvas-updates at 317c1ea4. Updated docs/ORCHESTRATION, AGENTS.md, docs/PROMPTS, and planner_suffix so authority is embedded docs + role prompts, machine deltas are mem preferences, and planner reports request gates instead of demanding local builds. Gates: cargo test --manifest-path src-tauri/Cargo.toml --lib prompts:: = 26 passed; cargo test --manifest-path cli/Cargo.toml cmd::docs = 6 passed.