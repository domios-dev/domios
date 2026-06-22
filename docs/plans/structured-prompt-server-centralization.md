# Plan — centralize structured-prompt-server allocation across ALL spawn paths

> Loop Lead scoping for the coordinator. Status: PLAN (not yet implemented). Requested decision:
> approve broad spawn-path refactor, or keep classifier permission-guard as the interim defense.

## Root cause (confirmed)
Only the orchestrator spawn path allocates the structured-prompt server. The block at
`src-tauri/src/service/orchestrator.rs:510–528`:
```
if provider.needs_structured_prompt_server() {
    if let Some(server) = structured_prompt::allocate_server() {        // reserve loopback port
        structured_prompt::register_endpoint(pane_id, base_url, cwd);   // record base_url
        params.structured_prompt_server = Some(server);                 // threaded into launch cmd
    }
}
```
Every OTHER spawn path constructs its launch params with `structured_prompt_server: None`:
`api/session_routes.rs:232`, `api/agent_routes.rs:82`, `service/transport.rs:106`,
`service/agent_restart.rs:210`, `service/spawn.rs:867`, `multiplexer/commands.rs:382`.

Consequence for a `needs_structured_prompt_server` provider (opencode) spawned via UI/direct/
restart/general/transport: no port bound ⇒ no SSE `/event` bus ⇒ (a) no structured `picker_question`
(permission/question boxes fall to the classifier, which false-idles a visible-cursor permission box —
see Round 11 guard), and (b) no HTTP control plane ⇒ no reliable abort (the r6 /stop blocker).

## Components in play
- `structured_prompt::allocate_server()` → `StructuredPromptServer{hostname, port}` (TOCTOU bind:0).
- `structured_prompt::register_endpoint(pane_id, base_url, cwd)` → records the endpoint.
- `structured_prompt::maybe_start_subscriber(svc, pane_id)` → idempotent (SUBSCRIBED set) SSE
  subscriber thread; reads base_url from the registered endpoint. **Must run for every path.**
- `build_agent_command` (orchestrator.rs:822) consumes `params.structured_prompt_server` to emit the
  port flag/env the agent binds. Two param structs carry the field: `SpawnParams` (orchestrator) and
  `AgentLaunchParams` (transport/restart/`build_launch_command`).

## Proposed change (capability-gated, no provider-id branch)
1. Add `structured_prompt::ensure_for_spawn(registry, agent_cmd, pane_id, cwd) -> Option<StructuredPromptServer>`
   that does the gate + allocate + register_endpoint (the extracted 510–528 logic). One source of truth.
2. Call it from the shared launch choke point so all paths benefit. Preferred: inside the params
   builder both struct types funnel through, OR a one-liner at each of the ~6 call sites that build a
   launch + spawn. Each site already has `pane_id` + `cwd` (verify per site).
3. Ensure `maybe_start_subscriber(svc, pane_id)` is invoked post-register on every path (today likely
   only orchestrator / first-output). Centralize alongside (1).
4. Keep the classifier permission-guard (Round 11) as defense-in-depth — but with the server present,
   opencode permission/question surfaces arrive as structured `picker_question` and never reach it.

## Risk (why this is plan-first / WAITING_FOR_COORDINATOR)
- Touches EVERY agent spawn path incl. **restart** and **general-agent** — a regression breaks all
  launches. Load-bearing.
- Port lifecycle: `allocate_server` TOCTOU + per-pane subscriber thread must not double-start on
  restart (SUBSCRIBED guards this, but restart reuses pane_id — verify no stale-endpoint reuse).
- `build_agent_command` must emit the bind flag/env identically for all paths (today only exercised
  via orchestrator).
- claude/codex/cursor are `needs_structured_prompt_server()==false` ⇒ unchanged (gate), but each must
  be re-confirmed.

## Recommendation
Contained in code size (1 helper + ~6 one-line call-site edits + 1 subscriber-start call), but
load-bearing. Implement in a worktree with: spawn-path unit coverage, a live "opencode via normal UI
spawn → /api/panes shows structured picker_question on a permission" smoke, and a restart smoke. The
opencode HTTP abort then builds on the now-present server base (separate follow-up). Awaiting
coordinator go/no-go before touching the shared spawn path.

<!-- revision: 1 -->
