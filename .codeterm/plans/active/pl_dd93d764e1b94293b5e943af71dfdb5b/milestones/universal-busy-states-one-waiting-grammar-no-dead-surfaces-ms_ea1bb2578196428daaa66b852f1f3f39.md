---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_ea1bb2578196428daaa66b852f1f3f39
handle: universal-busy-states-one-waiting-grammar-no-dead-surfaces-ms_ea1bb2578196428daaa66b852f1f3f39
title: status_update
---

IN_REVIEW: STATUS: in_review | BRANCH: lane/universal-busy-states. CHANGES: NEW shared/BusyState.tsx (BusyState dots+i18n label; BusySpinner sole animate-spin owner) + shared/busyStateLabels.ts (locale-keyed i18n) + agent-state.css ring/reduced-motion. Wired ChatView thinking, TerminalPane boot overlay (consumes optional pane.boot_stage w/ generic fallback), PlanFetchErrorState load+reconnect, RemoteFolderPicker, GlancePopover, MemoryItemEditor. Converted ALL animate-spin affordances to BusySpinner (Settings x5, SettingsMenu, PluginActivityInline, MemoryDreamsTab, UpdateBadge, UpdateProgressLayer, MeshPanel x2, AddPeerPanel x2 bespoke SVG/border deleted, help/AskButton). types.ts: added optional Pane.boot_stage. NEW BusyState.test.tsx + BusyState.grepgate.test.ts. TESTS: grep gate GREEN (zero leftovers outside allowlist), labels logic GREEN via tsx. BusyState.test.tsx authored but NOT run: lane worktree has NO node_modules (react/happy-dom absent) so React render tests defer to integration gate; did not install deps or run broad tsc per your note. FOLLOWUP: integration gate run npm test:unit + playwright -g busy; pane.boot_stage populated only after sibling task_943aa1484d49414fbcb49d44cd67f982 lands (generic label until then).
