# Orchestration Toolset

Full `codeterm plan` mechanics live in the embedded doc — read it on demand without polluting the system prompt:

```bash
codeterm docs ORCHESTRATION
```

The doc covers: the Plan → Sections → Tasks → Milestone-chains model; the lifecycle (create → kinds → sections → tasks → assign/spawn → report→milestone → close → loop); the complete `codeterm plan ...` command map; provenance nodes + edges (§3.8); live-mutability (§3.9); and what lives where.

Planning source of truth: CodeTerm GOAL/Spec/Plan/Tasks/Strides. Set GOAL first. Spec holds WHAT/WHY, Plan holds HOW, Tasks hold execution briefs. Do not create standalone markdown implementation plans as the primary planning surface unless the user explicitly asks for an export. Use mem (`codeterm mem wake-up` / `mem search`) to load machine/user orchestration preferences before shaping a plan.

Orchestration *principle* (when to spawn, model routing, discipline) lives in the mem skill `codeterm-orchestrator` — separate from mechanics by design.
