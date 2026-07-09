---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_af5688574a2c4fa99d209c00441d65ea
handle: prod-kill-guard-ms_af5688574a2c4fa99d209c00441d65ea
title: decision
---

INCIDENT: prod (7685) got killed. ROOT CAUSE: build.sh kill targets lsof on CT_PORT=${CODETERM_API_PORT:-7686}; an orchestrator/worker pane in PROD exports CODETERM_API_PORT=7685, so a bare ./build.sh kill nuked prod. FIX (committed 1932a235): hard guard right after CT_PORT resolution — any build.sh action aborts (exit 2) if resolved port==7685, with a message telling the caller to set 7686/instance. Tested: CODETERM_API_PORT=7685 ./build.sh kill now refuses, prod stays alive.
