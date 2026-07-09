---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_3fd9434142d240b5835a72fc770f78bf
handle: sidebar-ghosts-runtime-closed-panes-persist-as-live-child-rows-ms_3fd9434142d240b5835a72fc770f78bf
title: decision
---

ROOT CAUSE CONFIRMED LIVE (round-66, Planner cleaned the actual incident by hand): the ghost rows come from the DAEMON hierarchy links (parent->child), not frontend cache — pane death does not unlink; 'codeterm agent unlink --pane <id>' removed all 13 ghosts instantly (verified: pane list --agents now shows only 7 live children under the orchestrator). SCOPE REFINEMENT: the primary fix is DAEMON-SIDE — auto-unlink (or mark dead) hierarchy links when a pane closes/dies, so every consumer (sidebar, ledger liveness, watchdog) sees truth; the frontend structural-derivation requirement from the original body stays as defense-in-depth (render only panes present in state.panes). Note: 'close' on a ghost row no-ops because there is no pane to close — the fix should also make the row's close affordance perform unlink for dead-linked entries as a fallback. The durable ledger (agent ledger) correctly keeps history — do NOT purge it; only live LINKS must die with the pane.
