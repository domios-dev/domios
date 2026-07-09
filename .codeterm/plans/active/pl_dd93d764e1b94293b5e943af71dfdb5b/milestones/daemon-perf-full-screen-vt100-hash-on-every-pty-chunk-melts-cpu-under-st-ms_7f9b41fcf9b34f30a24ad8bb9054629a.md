---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_7f9b41fcf9b34f30a24ad8bb9054629a
handle: daemon-perf-full-screen-vt100-hash-on-every-pty-chunk-melts-cpu-under-st-ms_7f9b41fcf9b34f30a24ad8bb9054629a
title: status_update
---

IN_REVIEW: STATUS: in_review
BRANCH: lane/3a7c7e8e-hash-debounce @ 4884e9d6 (base a4b328dd = current integration head)
WORKTREE: /Users/rollacode/Developer/codeterm-lanes/3a7c7e8e-hash-debounce
CHANGES — two profiled perf fixes, Andrey-verified problem chain:
1. perf(chat) b6399ac7: HIDDEN CHAT PANES STOP THE DAEMON PARSER. Root cause: useChatSession called chat_start + subscribed on mount for EVERY mounted chat pane regardless of visibility — isVisible was plumbed into PaneSessionChat but the expensive subscription ignored it; background tabs kept the daemon vt100 parsing + frontend re-rendering forever (matches the live profile: vt100::Row::write_contents hot leaf at 286% daemon CPU; Andrey confirmed empirically — switching all panes off chat made the machine instantly fine). Fix: useChatSession gains active param (default true), effect early-returns when inactive and chat_stop fires via cleanup on visibility loss; PaneSessionChat passes active=isActive. Messages re-fetch on re-activation via chat_start.
2. perf(plan) 4884e9d6: FREEZE HIDDEN PLAN SURFACES. PlanViewShell mounts board+list+canvas simultaneously (hidden attr) and spread live {...props} into all three — React.memo passed on every plan tick because props genuinely changed, so THREE full surface renders per tick, permanently. Fix: useFrozenWhenHidden hook — hidden surfaces keep their last-visible props object (identical references -> memo skips entirely); re-shown surface gets fresh props in the same render. Only the visible view now pays for live plan updates.
TESTS RUN: none (Andrey's explicit order: write tests, do not run — speed over local verification)
TESTS WRITTEN: PlanViewShell.test.tsx +1 (freeze keeps last-visible reference / fresh on re-show); useChatSession active-gating relies on the existing chat_start/chat_stop seam — recommend QA adds an assertion that chat_start is NOT invoked when active=false (test seam file style didn't fit a hook lifecycle test in the window).
GATES REQUESTED: tsc; existing PlanViewShell (4/4) + useChatSession + PaneSessionChat suites; new freeze test; live drill on dev: open 5 agent chats in background tabs -> daemon CPU must NOT climb (the before-condition reproduces in minutes on the live plan); plan-tick render-count spot-check via React profiler if cheap.
FOLLOWUP: (1) the daemon-side content-hash debounce from this task's body is DEFERRED — correctness needs the idle-poll interplay verified (a burst ending inside the debounce window must still settle status via idle-poll backstop); the frontend fixes remove the dominant load Andrey reproduced; re-profile after these land and file the debounce separately if the vt100 leaf is still hot; (2) task_1d0572d0 invariants lane: analysis complete in this session (#209 root cause = update_task_lane fill-if-empty on assigned_pane_id at task_ops.rs:233; RPC orders update_task BEFORE lane so same-command pane+status needs reorder), worktree lane/7140a28c-state-invariants created, NO code committed — hand to the next worker with these findings.