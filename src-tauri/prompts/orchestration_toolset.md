# Orchestration Toolset

Full `codeterm plan` mechanics live in the embedded doc — read it on demand without polluting the system prompt:

```bash
codeterm docs ORCHESTRATION
```

The doc covers: the Plan → Sections → Tasks → Milestone-chains model; the lifecycle (create → kinds → sections → tasks → assign/spawn → report→milestone → close → loop); the complete `codeterm plan ...` command map; provenance nodes + edges (§3.8); live-mutability (§3.9); and what lives where.

Orchestration *principle* (when to spawn, model routing, discipline) lives in the mem skill `codeterm-orchestrator` — separate from mechanics by design.
