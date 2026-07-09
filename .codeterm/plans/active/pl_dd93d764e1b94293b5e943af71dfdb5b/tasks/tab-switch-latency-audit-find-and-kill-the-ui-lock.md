---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_fd79beb6aff24c51a2ffb784164b575e
handle: tab-switch-latency-audit-find-and-kill-the-ui-lock
title: 'Tab-switch latency audit: find and kill the UI lock'
status: done
---

User reports consistent slow loading + a feeling of a UI lock when clicking tabs (round-12). Known past incident class (AGENTS.md async runtime discipline): a heavy/blocking Tauri command on tab-switch starves the whole IPC runtime — plugin_manager_snapshot once froze the UI 3-4s per tab switch by forking CLI subprocesses. Current dev log shows zero SLOW-IPC entries since restart, so the culprit is either frontend (heavy synchronous mount/render on activation) or an IPC starvation that needs load to reproduce. AUDIT: (1) enumerate EVERY invoke fired on tab activation (grep activation path: setActiveTab effects, TabContent/TerminalPane/ChatView mounts) and log timings; (2) use the existing [SLOW-IPC]/[LONGTASK] instrumentation under realistic load (many panes, the live plan workspace) — reproduce the lock, capture which command resolves FIRST slowly (that one is the cause, the rest are queued victims); (3) frontend profile: React commit times on tab switch (xterm re-mount? chat history re-parse? mission-control model rebuild on plan tabs?); (4) fix the top offenders: cache snapshots, move blocking work to spawn_blocking, memoize mounts, virtualize history. Acceptance is measured, not vibes.
