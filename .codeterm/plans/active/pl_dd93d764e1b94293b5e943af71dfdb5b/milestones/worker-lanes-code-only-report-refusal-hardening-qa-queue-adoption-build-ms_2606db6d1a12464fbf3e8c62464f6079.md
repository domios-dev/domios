---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_2606db6d1a12464fbf3e8c62464f6079
handle: worker-lanes-code-only-report-refusal-hardening-qa-queue-adoption-build-ms_2606db6d1a12464fbf3e8c62464f6079
title: decision
---

INVARIANT SHARPENED (round-70, user restated as product-general): ONE QA GATE PER MACHINE. Every daemon instance registers exactly ONE QA lane/pane; ALL heavy gates on that machine route through it (Mac-Air local QA, eight macOS QA, ROG Windows QA, ubunta Linux QA — but expressed in product terms: per-daemon singleton, works in any deployment). ADD TO THIS TASK'S SCOPE (fix 4 extension): (a) daemon-side QA-pane registry — one QA-flagged pane per instance, registered explicitly (codeterm plan qa --claim-runner or equivalent), re-registration replaces (never two); (b) 'plan qa --task' routes execution to the machine's registered QA pane (or refuses with 'no QA runner registered' + the claim command); (c) the build-detection exemption keys on the REGISTRY (the one registered pane), not on a title convention — mesh drills: a gate requested for a remote-peer lane routes to THAT peer's QA pane via --mesh; (d) acceptance drill: two panes attempt --claim-runner -> second replaces first with a stride; a worker pane running cargo while a QA pane is registered -> nudged; the QA pane itself -> never nudged. Product-general by construction: registry is per-daemon, no machine names in code.
