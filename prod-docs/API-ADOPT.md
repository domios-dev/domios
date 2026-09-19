# Tab Agent Adopt

REST contract for `POST /api/tabs/{id}/adopt-agent`, the verb behind
`codeterm tab adopt-agent`.

`codeterm tab adopt-agent --provider <id>` adopts a live PTY as an agent send
target without restarting or killing it. `--provider` is required and must
name a registered PTY agent; adopt never infers or silently invents
`agent_type`. After a successful adopt, `codeterm send --from-pad` delivers a
routed agent message. `--to-shell` is not the adopt path.

A login shell or a screen with no agent TUI is refused unless the owner
passes `--force`. Caller proof comes from `X-CodeTerm-Tab-Id` and
`X-CodeTerm-Binding-Nonce`. Direct user (no caller tab), a proven Team
anchor, the target tab itself, or the live Plan owner of the target's bound
task may adopt. Every accepted identity write is journaled with origin
`adopt`, previous/new provider, and caller tab.

`POST /api/tabs/{id}/adopt-agent` body: `{provider, force?}`. Response:
`ok`, `tab_id`, `pty_id`, `provider` as `{status, previous, current}` where
`status` is `kept` or `corrected`, `caller_tab_id`, and `force`. Refusals use
stable `tab_adopt_*` codes, including `tab_adopt_not_authorized`,
`tab_adopt_agent_tui_required`, and `tab_adopt_unknown_provider`. The PTY id
is unchanged.

<!-- revision: 1 -->
