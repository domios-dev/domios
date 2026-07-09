---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_297cc06d2aaf45f391ebc3e907ef16e8
handle: full-regression-on-ubunta-linux-via-mesh-run-don-t-trust-ms_297cc06d2aaf45f391ebc3e907ef16e8
title: status_update
---

linux-settings-stale-stack — integrated ubunta fix/settings-stale-stack @ d215ac05. SettingsPanel now resets route stack to root when opened without initialSection. Gates: Settings sections test, SchemaRenderer test, tsc, pre-push shell/cargo/fmt/clippy. ubunta smoke without lmstudio reached 23 pass / 3 fail; next Linux blocker is 13-settings keybinding casing, deferred.
