---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_9a92b84a0c2f4db792644e8a78ea4d75
handle: full-regression-on-rog-windows-via-mesh-run-don-t-trust
title: Full regression on ROG (Windows) via mesh — run, don't trust
status: done
---

Run the FULL regression suite on mesh peer ROG (paired, Windows, a large GPU; codeterm --mesh ROG works from here). Checkout branch feat/canvas-updates at the current integration sha (record the sha first: git rev-parse HEAD). Steps, all through the repo's own machinery: (1) .\build.ps1 check then Rust tests via the capped runner — narrow filters per module, NEVER one giant bare cargo test; PowerShell foreground, 90s cap per run (compile phases may need TEST_CAP_SECS=420 — a cap fire during compilation is not a test failure, rerun; a cap fire during test execution IS a finding). (2) Start dev instance (port 7686, CODETERM_INSTANCE if the default is taken), verify /api/health build sha matches checkout. (3) Playwright FULL project over CDP against the real WebView2 desktop app (Windows is the only platform that tests native desktop behavior) — mandatory --timeout=30000 --global-timeout=600000. (4) Windows-specific smoke: ConPTY panes open/close, PowerShell OSC7 cwd, key handling e.code. OUTPUT CONTRACT: every failure = a stride on this task with test name, full error excerpt, repro command, suspected area — and a fix task filed with a cheap-model-ready body. Zero failures = attach run logs/counts as evidence. NEVER mark done with skipped suites — list every suite skipped and why.
