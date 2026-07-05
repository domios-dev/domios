---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_29e9c4f0f55e4748a9bc3783bb7e4c56
handle: settings-guide-how-to-panel-i18n-ready-ms_29e9c4f0f55e4748a9bc3783bb7e4c56
title: status_update
---

STATUS: done
BRANCH: lane/settings-help-guide
SHA: a620fff6cc0291447a46291a54ce59e4e1d6c751
CHANGES: src/help/* (types, en content, figures, HelpPanel, HelpBlockView, shortcuts); Settings sections.ts + SettingsPanel.tsx (guide section); sections.test.ts + help unit tests
BUILD/TYPECHECK/TESTS: git diff --check pass; npx tsx src/help/content.test.ts; src/help/shortcuts.test.ts; src/help/HelpPanel.test.tsx; src/components/Settings/sections.test.ts — all green. Broad build/tsc not run per task scope.
FOLLOWUP: App.tsx could accept initialHelpSection for deep-linking (e.g. open Settings → Guide → Plans). SettingsMenu already lists guide via SETTINGS_SECTIONS — no separate menu edit needed. Future locales: add src/help/content/ru.ts and register in content/index.ts.
