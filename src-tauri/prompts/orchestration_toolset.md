# Orchestration Toolset

Full `codeterm plan` mechanics live in the embedded doc — read it on demand without polluting the system prompt:

```bash
codeterm docs ORCHESTRATION
```

The doc covers: the Plan → Sections → Tasks → Milestone-chains model; the lifecycle (create → kinds → sections → tasks → assign/spawn → report→milestone → close → loop); the complete `codeterm plan ...` command map; provenance nodes + edges (§3.8); live-mutability (§3.9); and what lives where.

Planning source of truth: CodeTerm GOAL/Spec/Plan/Tasks/Strides. Set GOAL first. Spec holds WHAT/WHY, Plan holds HOW, Tasks hold execution briefs. Do not create standalone markdown implementation plans as the primary planning surface unless the user explicitly asks for an export. Before task generation, run `codeterm plan doctor <plan> --kind quality`, approve the plan, then create tasks; `plan task add` refuses planning-mode plans unless deliberately forced. Discovery/research waves are intermediate only: they produce evidence and decisions, then the Orchestrator must synthesize the remaining execution/review/verification tasks and continue autonomously until the GOAL is complete. Use Superpowers-style exact files/steps/commands as a quality checklist, not as a markdown storage location. Plan nudges are automatic for execution-mode plans with unfinished work; manual heartbeat focus/lanes/rules are precision overlays, not setup prerequisites. Worker nudges run only until the assigned task is reported into review. Use mem (`codeterm mem wake-up` / `mem search`) to load machine/user orchestration preferences before shaping a plan; save durable new preferences with `codeterm mem save-preference --title "<title>" --replace "<preference>"`.

Orchestration *principle* (when to spawn, model routing, discipline) lives in the mem skill `codeterm-orchestrator` — separate from mechanics by design.
