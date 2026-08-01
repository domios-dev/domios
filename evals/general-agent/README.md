# General Agent provider matrix

This bench validates the real first-run General Agent path against a clean dev
profile. It uses only the authenticated dev API on port 7686 and subscribes to
SSE before launch. It never targets prod, invokes Playwright, polls, sleeps, or
treats elapsed time as successful readiness.

Run one provider per newly created `~/.codeterm-dev` profile:

```powershell
$env:GA_PROVIDER='claude'; npm run test:general-agent-provider
$env:GA_PROVIDER='codex'; npm run test:general-agent-provider
$env:GA_PROVIDER='opencode'; npm run test:general-agent-provider
$env:GA_PROVIDER='cursor'; npm run test:general-agent-provider
```

Claude, Codex, and OpenCode must:

1. replace the untouched Get started surface;
2. bind an authoritative session and confirm bootstrap delivery;
3. produce a first assistant reply;
4. restart with the same Tab and session, a new PTY, and produce an exact
   post-resume reply.

Cursor is intentionally launch-only because a limited account may not produce
a model reply. It must still reach `agent_ready`, bind a session, and confirm
the combined system/bootstrap delivery.

The runner refuses a reused profile. Before each invocation, stop dev with
`.\build.ps1 kill`, preserve or remove only `~/.codeterm-dev`, then start a
fresh visible dev instance with `.\build.ps1 dev`. Never remove
`~/.codeterm`, which is prod.

<!-- revision: 1 -->
