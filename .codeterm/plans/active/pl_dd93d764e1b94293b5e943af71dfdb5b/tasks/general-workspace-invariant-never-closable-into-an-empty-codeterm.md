---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_10a20efae2174a86bc2f964ef82dea09
handle: general-workspace-invariant-never-closable-into-an-empty-codeterm
title: 'General workspace invariant: never closable into an empty CodeTerm'
status: done
---

USER-REPORTED HOLE: there is a state where the 'choose new General agent' tab (FirstRunGeneralPicker) exists, and closing it leaves CodeTerm completely EMPTY — no General workspace at all. Make this state UNREACHABLE. THE INVARIANT (enforce at every layer): (1) the General workspace ALWAYS exists — it is protected: not closable, not deletable from UI, CLI, or API; if anything removes it, the daemon recreates it (backend lead: src-tauri/src/service/general_agent/workspace.rs ensure_* already creates it at startup and dedupes duplicates — extend so it re-fires on workspace-close events, not only startup); (2) the General workspace ALWAYS contains at least one of: the agent-picker tab (src/components/AgentPicker/FirstRunGeneralPicker.tsx) OR a live General agent pane — closing the picker tab while it is the last content must either be blocked (close affordance hidden on last-tab-in-general) or instantly respawn the picker; closing the General agent pane falls back to showing the picker again, never to emptiness. AUDIT ALL CLOSE PATHS: tab close button, close-others/close-all actions, keyboard shortcut, session-restore edge (restore with zero panes must land on picker, not blank), workspace close via any sidebar/menu/CLI route, and the reducer fallbacks (CLOSE_SESSION_PICKER picks remaining[last] — verify what happens when remaining is empty in the General workspace). Add a state-level guard: a single reducer/store invariant check (dev-mode assert + production auto-repair) that General workspace exists with >=1 tab, so future regressions self-heal instead of blanking the app.
