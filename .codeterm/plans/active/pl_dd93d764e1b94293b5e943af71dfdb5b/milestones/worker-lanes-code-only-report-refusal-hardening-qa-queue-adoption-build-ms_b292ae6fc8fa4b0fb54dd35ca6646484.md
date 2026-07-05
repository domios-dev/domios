---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_b292ae6fc8fa4b0fb54dd35ca6646484
handle: worker-lanes-code-only-report-refusal-hardening-qa-queue-adoption-build-ms_b292ae6fc8fa4b0fb54dd35ca6646484
title: decision
---

PROCESS PREFERENCE CLARIFIED (round-86, Orchestrator + Andrey): PARALLEL QA ACROSS MACHINES IS ALLOWED AND ENCOURAGED — e.g. Mac-Air local QA (orchestrator-controlled) + eight QA lane running simultaneously. The invariant was never 'one QA total': it is ONE HEAVY GATE PER PHYSICAL MACHINE, and the per-daemon registry design already encodes exactly that (each machine registers its own single QA pane; N machines = N parallel gates). No design change needed — this stride exists so nobody later misreads the invariant as global serialization. COMPANION RULE (binds to the cross-machine etiquette TEXT B, now on the prompt lane): when a fix belongs on another machine, spawn a REAL worker pane inside CodeTerm on that peer (workspace + agent spawn --mesh) — never drive it through ad-hoc chat messages; real panes get lane bindings, reports, watchdog coverage, and QA routing; chat-driven work is invisible to every mechanism we built.
