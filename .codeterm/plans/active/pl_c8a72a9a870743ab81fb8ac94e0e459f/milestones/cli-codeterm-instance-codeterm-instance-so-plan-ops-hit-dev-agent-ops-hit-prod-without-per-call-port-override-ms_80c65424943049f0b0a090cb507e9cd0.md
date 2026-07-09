---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_80c65424943049f0b0a090cb507e9cd0
handle: cli-codeterm-instance-codeterm-instance-so-plan-ops-hit-dev-agent-ops-hit-prod-without-per-call-port-override-ms_80c65424943049f0b0a090cb507e9cd0
title: decision
---

centralized instance resolution in cli/src/instance.rs (Instance enum prod/dev/Named); --instance flag clears inherited CODETERM_API_PORT/TOKEN/SOCKET so it overrides ambient pane env; precedence flag > explicit port/token env > CODETERM_INSTANCE env > config.toml > default prod. @a6197876 on canvas/cli-instance. GATE DEFERRED: load 71 (Fermi building) — cargo check/test cli crate + manifest::cli_manifest_is_fresh pending low load.
