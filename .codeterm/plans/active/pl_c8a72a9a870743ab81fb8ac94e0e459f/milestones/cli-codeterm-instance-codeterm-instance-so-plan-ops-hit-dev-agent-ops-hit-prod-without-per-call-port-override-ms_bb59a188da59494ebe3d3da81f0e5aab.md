---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_bb59a188da59494ebe3d3da81f0e5aab
handle: cli-codeterm-instance-codeterm-instance-so-plan-ops-hit-dev-agent-ops-hit-prod-without-per-call-port-override-ms_bb59a188da59494ebe3d3da81f0e5aab
title: testing
---

ACCEPTED gated. cli crate (codeterm-cli) compiles clean + 215 tests PASS 0 fail (incl cli::instance canonical-names/dir/port/socket + cli::client deterministic_socket). New instance.rs Instance enum + --instance flag + CODETERM_INSTANCE; flag clears inherited port/token so it overrides ambient pane (flag-overrides-env deviation ACCEPTED — literal brief would break --instance dev from inside a pane). @a6197876 on canvas/cli-instance. Fixes the dev/prod port footgun.
