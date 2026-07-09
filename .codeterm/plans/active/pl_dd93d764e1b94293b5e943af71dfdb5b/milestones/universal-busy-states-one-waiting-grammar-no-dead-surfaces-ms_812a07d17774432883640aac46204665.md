---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_812a07d17774432883640aac46204665
handle: universal-busy-states-one-waiting-grammar-no-dead-surfaces-ms_812a07d17774432883640aac46204665
title: status_update
---

IN_REVIEW: STATUS: in_review
BRANCH: lane/universal-busy-states

CHANGES:
- NEW src/components/shared/BusyState.tsx — shared waiting grammar. BusyState (ct-phase-dots canon motion + i18n label, inline/block, hideLabel, phase) and BusySpinner (icon/ring affordance, SINGLE owner of animate-spin).
- NEW src/components/shared/busyStateLabels.ts — locale-keyed i18n constants (getBusyLabels/busyLabel), mirrors help-content pattern.
- src/styles/agent-state.css — added .ct-busy-ring + reduced-motion neutralization for .ct-busy-spinner (dots already covered).
- Wired waiting surfaces: ChatView thinking, TerminalPane boot overlay (consumes optional pane.boot_stage, generic fallback), PlanFetchErrorState load + reconnect, RemoteFolderPicker, GlancePopover, MemoryItemEditor.
- Converted ALL animate-spin affordances -> BusySpinner: Settings (Logs/Update/Secrets/Plugins x2), Titlebar SettingsMenu, Extensions PluginActivityInline, MemoryDreamsTab, UpdateBadge, UpdateProgressLayer, MeshPanel x2, AddPeerPanel x2 (bespoke SVG/border spinners deleted), help/AskButton.
- src/lib/types.ts — added optional Pane.boot_stage?: string|null (tiny type change to consume sibling backend boot-stage events).
- NEW BusyState.test.tsx (markup contract, i18n labels, explicit-stage override, hideLabel a11y, spinner spin/static/ring, reduced-motion CSS assertion) + BusyState.grepgate.test.ts.

BUILD/TYPECHECK/TESTS:
- Grep gate GREEN: npx tsx BusyState.grepgate.test.ts -> zero animate-spin/Loading.../Loading… outside allowlist (BusyState.tsx + its 2 test files). Confirmed by raw grep too.
- Labels logic GREEN via focused tsx.
- BusyState.test.tsx AUTHORED but NOT run here: this lane worktree has NO node_modules (react/happy-dom absent) — React render tests need the integration gate. Per your 'no broad builds' note I did not install deps or run tsc.

FOLLOWUP:
- Integration gate: run npm test:unit (BusyState.test.tsx + PlanFetchErrorState.test.tsx) with node_modules present; run playwright -g busy for spawn busy-flow + throttled plan load.
- pane.boot_stage is consumed but only populated once sibling task_943aa1484d49414fbcb49d44cd67f982 (daemon boot-stage events) lands; until then boot overlay shows generic 'Starting session'.
- Did not touch NewSessionPicker/first-run install spinners (none used animate-spin/Loading tokens); flag if you want those surfaced through BusyState too.
