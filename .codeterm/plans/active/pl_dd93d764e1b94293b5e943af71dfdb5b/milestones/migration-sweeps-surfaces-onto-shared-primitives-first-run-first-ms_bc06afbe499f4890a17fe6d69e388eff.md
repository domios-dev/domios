---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_bc06afbe499f4890a17fe6d69e388eff
handle: migration-sweeps-surfaces-onto-shared-primitives-first-run-first-ms_bc06afbe499f4890a17fe6d69e388eff
title: status_update
---

Accepted and merged in 9abe0df4 (first-run + new-session primitive migration).

Changes accepted:
- First-run provider picker CTA now uses shared Button primary/lg with BusyState loading.
- Provider status badges use Chip; install panel uses Card; install/re-check actions use Button.
- New-session picker and orchestration modal actions use shared Button variants.
- OrchestrationCreate tests were updated to the real DropdownSelect contract; the stale native-select assumptions are gone.
- Integration safety fix: shared Button now defaults to type="button".

Gates:
- npx tsx src/components/Sidebar/OrchestrationCreate.test.tsx: 5/5 pass
- npx tsx src/components/Sidebar/OrchestrationPairForm.test.tsx: 2/2 pass
- npx tsx src/components/shared/Button.test.tsx: 9/9 pass
- npx tsx src/components/shared/shared.grepgate.test.ts: pass; bespoke chip 38->22, bespoke inline button 26->16
- npx tsc --noEmit: pass
- pnpm build: pass
- picker golden non-spawn scenarios: 6/6 pass under --timeout=30000 --global-timeout=600000
- live picker screenshot captured: /tmp/new-session-picker-primitives.png
- git diff --check excluding .codeterm sidecars, file-size gate, comment-block gate: pass
- pre-push shell guards, cargo check, fmt, clippy: pass

Note: first-run synthetic screenshot could not be rendered through the live shell without mutating first-run config; accepted via component/code/build evidence plus picker visual. The next sweep families remain Settings, Sidebar, plan views, and chat chrome.
