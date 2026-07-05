---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_a4c6b285a11d46dd97fc98a25267f285
handle: crate-split-exploration-cut-src-tauri-compile-times-with-workspace-bound
title: 'Crate split exploration: cut src-tauri compile times with workspace boundaries'
status: done
---

The community-standard structural fix for slow Rust iteration is splitting the monolith crate along stable seams so incremental builds recompile only the changed crate and the linker does less work (crates/mem-store and codeterm-plugins already exist as precedent). RESEARCH TASK — evidence first: (1) run 'cargo build --timings' on src-tauri (dev profile, warm) and attach the HTML report; identify the top compile-time units and the critical path; (2) map 2-4 candidate seams with LOW coupling (candidates to evaluate, not prescribe: plan store + planner domain, mesh, usage/telemetry, agent providers core) — for each: files, pub surface, circular-dep risks; (3) prototype ONE split (the best ratio of compile-time saved to churn) behind a workspace member and measure before/after: warm incremental rebuild after touching a file in the split area, and full cold build; (4) recommendation with numbers — proceed to a split wave or stop (honest 'not worth the churn' is a valid outcome). Constraints: no behavior changes, no pub-surface redesigns beyond what the move forces; file-size discipline applies to any new mod.rs.
