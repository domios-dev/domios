---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_c979592e876847c686f48020d3553a48
handle: fix-replace-hardcoded-general-agent-chat-view-provider-allowlist-with-ca
title: 'fix: replace hardcoded General Agent chat-view provider allowlist with capability lookup'
status: done
tags:
- review-r2
- rust-daemon
---

HIGH finding from R2 Rust daemon core review. In src-tauri/src/service/general_agent/workspace.rs, default_chat_view_for_provider uses BUNDLED_CHAT_UI_PROVIDERS = [claude,codex,opencode,cursor] and passes contains(provider_id) into should_default_agent_pane_to_chat. Core should branch on AgentProvider::supports_chat_ui() or a registry capability, not provider ids. Impact: a plugin/drop-in provider that supports chat UI will default the General Agent pane to terminal during first-run/general workspace setup, while a renamed/forked provider can drift from behavior. Fix by threading AgentRegistry or a capability boolean from the caller and adding a regression test with a non-bundled chat_ui-capable provider.
