# Orchestration Toolset

Full `codeterm plan` mechanics live in the embedded doc — read it on demand without polluting the system prompt:

```bash
codeterm docs ORCHESTRATION
```

The doc covers: the Plan → Sections → Tasks → Milestone-chains model; the lifecycle (create → kinds → sections → tasks → assign/spawn → report→milestone → close → loop); the complete `codeterm plan ...` command map; provenance nodes + edges (§3.8); live-mutability (§3.9); and what lives where.

Planning source of truth: CodeTerm GOAL/Spec/Plan/Tasks/Strides. Set GOAL first. Spec holds WHAT/WHY, Plan holds HOW, Tasks hold execution briefs. Do not create standalone markdown implementation plans as the primary planning surface unless the user explicitly asks for an export. Before task generation, run `codeterm plan doctor <plan> --kind quality`, approve the plan, then create tasks; `plan task add` refuses planning-mode plans unless deliberately forced. Exception: `plan task add --type planning` is first-class — planning-kind tasks (improve GOAL/spec/plan/task-graph) are allowed on a planning-mode plan without `--force` and never trip the doctor's `tasks_before_approval` gate. Discovery/research waves are intermediate only: they produce evidence and decisions, then the Orchestrator must synthesize the remaining execution/review/verification tasks and continue autonomously until the GOAL is complete. Use Superpowers-style exact files/steps/commands as a quality checklist, not as a markdown storage location. Plan nudges are automatic for execution-mode plans with unfinished work; manual heartbeat focus/lanes/rules are precision overlays, not setup prerequisites. Worker nudges run only until the assigned task is reported into review. Use mem (`codeterm mem wake-up` / `mem search`) to load machine/user orchestration preferences before shaping a plan; save durable new preferences with `codeterm mem save-preference --title "<title>" --replace "<preference>"`.

Planning ownership: while a Planner holds an open planning-kind task, route requirements/blockers/evidence to the Planner instead of mutating graph nodes (goal, sections, tasks) yourself. You may record strides, heartbeat updates, and lane metadata. If you must mutate the plan in an emergency, record it as a stride so the Planner can reconcile it. The CLI will warn you with a one-line hint when the active planner lane belongs to a different pane.

Orchestration *principle* (when to spawn, model routing, discipline) lives in the mem skill `codeterm-orchestrator` — separate from mechanics by design.
