# CodeTerm Vision

## What it is

CodeTerm is a **distributed OS for AI agents**. Not a terminal with AI bolted on — it's infrastructure that agents need to live, work, and coordinate, wrapped in a terminal because that's where developers work. What exists today, capability by capability, is mapped in `essences/capabilities.md`.

## Core metaphor: spinal cord

The CodeTerm CLI and APIs are the **spinal cord** — low-level reflexes that any agent uses to act within the environment. An agent doesn't need to understand how tabs, workspaces, or mesh work internally. It just uses the tools: open a tab, read output, spawn a worker, send a message.

The **General Agent** is the brain of one machine — manages the environment, routes messages, delegates work. It is one agent among many. Other agents get the spinal cord (the `/codeterm` control surface) but never the General Agent's identity or personality.

## Meta-teams

CodeTerm teams are meta-teams: their leads shape the agents that do the work.

- **The Orchestrator coordinates and verifies at speed.** It moves between workers, dispatches, integrates, checks results, notices what is wrong and reorients quickly.
- **The Planner designs the tasks.** It looks at each task deeply and gives its Worker a task-fitted personality and role framing that narrows the Worker's focus, the way "act as an orchestrator" in the first words shapes a model.
- **The task carries the personality**: its executor role, its role guidance, and a one-word name given once at spawn. Choosing the right personality for the task is a core orchestration skill, and model choice follows the same logic: reasoning-heavy work goes to the stronger reasoner, precise execution to the executor model.
- **Workers feel alive**: stable names, character, and a presentation that shows them working and cooling down.
- **Prompts stay short and unambiguous** so the task-specific framing dominates; generic boilerplate that dilutes it is removed.

## Agent-agnostic

Model-agnostic, provider-agnostic. Claude, Codex, local models, custom agents — anything that runs in a terminal. CodeTerm provides the substrate, not the intelligence. Each agent type gets its own provider for detection and integration, but the tools are universal.

## Isolation principle

Each agent has its own scope:
- **General Agent** — machine-level brain in the protected General workspace
- **Project agents** — live in project workspaces, get the `/codeterm` control surface, and don't inherit the General Agent's role or knowledge
- **Workers** — task-scoped: their identity is the one their task gives them; they report and finish

CodeTerm provides the environment and the control surface; a Worker's character comes from its task, never from another agent.

## Mesh: distributed fabric

Today: machines discover each other, pair into one flat trusted mesh, and relay commands, agents and reports via `--mesh`.

Tomorrow: the mesh becomes a **unified fabric** where:
- **Shared memory** — agents across machines share knowledge, context, and recall.
- **Project awareness** — any agent can ask "where is the backend repo?" and get routed to the right machine.
- **Unified delegation** — "build this" finds the best machine (has the GPU, has the repo, is idle) without the user naming a peer.

## Plugins

Core stays small: one service layer over PTY, agents, plans, the message gateway, mesh, memory and the plugin host, reached by the UI and CLI clients (named in `essences/core.md`). Features — agent providers, bridges, secret backends, views — are plugins with declared capabilities. Provider/plugin architecture: `essences/contributing/architecture.md`.

## Future capabilities

- **Project indexing** — each machine indexes its projects into memory, queryable across the mesh.
- **Mini LLMs** inside CodeTerm for fast local classification, routing, and summarization.
- **Context-dependent recall** — knowledge surfaces by project, task, or person, not all at once.

## Design principles

1. **Spinal cord, not brain** — CodeTerm provides reflexes (tools), not intelligence (models)
2. **Agent-agnostic** — never hardcode for one provider
3. **Isolation** — agents don't leak into each other's identity or scope
4. **Fitted personalities** — every Worker starts from the framing its task needs
5. **Distributed-first** — every feature should work across mesh, not just locally
6. **Terminal-native** — the terminal is the universal interface; don't fight it, enhance it
7. **Progressive capability** — works standalone, gets better with mesh, gets smarter with memory

```map
anchors:
  - vector:agents-live-and-work-in-tabs
  - vector:agents-coordinate-as-meta-teams-through-plans
  - vector:work-flows-through-isolated-lanes-into-canonical
  - vector:machines-join-a-mesh
  - vector:people-reach-agents-through-bridges-voice-and-the-web
  - vector:the-host-owns-memory-secrets-plugins-and-configuration
  - vector:the-product-observes-and-updates-itself
```

