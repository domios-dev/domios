---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_7d9c444dae2d41dba77320dea11cd23b
handle: fix-replace-hardcoded-general-agent-chat-view-provider-allowlist-with-ca-ms_7d9c444dae2d41dba77320dea11cd23b
title: status_update
---

Accepted and merged through ce525b51. General Agent first-run/startup chat default now uses AgentProvider::supports_chat_ui via AgentRegistry instead of bundled provider-id allowlist; added non-bundled chat-UI-capable provider regression. Integration fix added missing general_agent re-export. Gates: general_agent_chat_default_uses_provider_capability_not_bundled_id PASS under scripts/test-capped.sh; npx tsc --noEmit PASS earlier in batch; cargo fmt/check/clippy PASS via pre-push; file-size/diff gates PASS.
