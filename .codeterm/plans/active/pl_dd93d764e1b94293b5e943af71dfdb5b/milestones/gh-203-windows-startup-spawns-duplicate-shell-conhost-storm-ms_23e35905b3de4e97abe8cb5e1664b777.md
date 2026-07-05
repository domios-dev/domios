---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_23e35905b3de4e97abe8cb5e1664b777
handle: gh-203-windows-startup-spawns-duplicate-shell-conhost-storm-ms_23e35905b3de4e97abe8cb5e1664b777
title: verified-already-fixed
---

ROG worker 6324354c verified GH #203 fixed on origin/feat/canvas-updates@f9c22a3c. Cold dev restore + kill/restart: 1 real shell, hidden/agent panes did not spawn shells, 0 legacy conhost.exe 0x4 under PTY shell; code review confirmed ConPTY SW_HIDE, terminalOwnership gate, Windows regression tests, no_window process utility coverage. No code changes.
