---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_55af14245f7e41ffa77e208a78f46251
handle: remote-qa-builders-git-backed-gate-execution-across-registered-mesh-peer-ms_55af14245f7e41ffa77e208a78f46251
title: review
---

Bounced internal QA runner at 5c5a4919. Env sanitization fixed underlying shell-guards (QA_GATE status=pass), but runner exits 1 and omits QA_RUN summary for a passing single gate, likely because qa_report_gate returns nonzero on empty detail under set -e. Required fix: reporting helpers return 0 and passing single gate exits 0 with QA_RUN.