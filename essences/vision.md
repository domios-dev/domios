# CodeTerm Vision

## What it is

CodeTerm is a **distributed OS for AI agents**. Not a terminal with AI bolted on — it's infrastructure that agents need to live, work, and coordinate, wrapped in a terminal because that's where developers work. The product name remains **CodeTerm**; the spinal cord names a product idea within it. What exists today and what is planned, capability by capability, is mapped in the [capability map](capabilities.md).

## Core metaphor: spinal cord

CodeTerm's **spinal cord** combines a deterministic control surface with small, bounded judgments. The CLI and APIs let any agent open a tab, read output, spawn a worker or send a message without knowing the host's internals. The Supporter Agent adds the OS reflexes that keep work moving when nobody is watching every terminal: ranking memory, naming tab states, classifying event-level states before heuristic fallback, helping recover the General Agent and coordinating provider updates. Planned Sentinels let agents watch for changes and route outcomes without spending their own turns polling.

The spinal cord accepts a class of decision models through a shared capability contract. Jev is the current reference model, not a required supplier. A connected decision adapter, including a suitable local adapter, responds to events with shared request accounting and visible spend. The intended default has no request cap; an explicit positive cap can bound calls. There are no feature switches, and without an adapter CodeTerm behaves exactly as before. **The model names the state; deterministic evidence authorizes the action.** The [Supporter contract](supporter-agent.md) owns that boundary and its hard recovery limits.

The **General Agent** remains the brain of one machine: it manages the environment, routes messages and delegates work. Project agents retain their own identity and purpose. Small host judgments keep the environment usable without taking over an agent's task or the owner's decisions.

## Meta-teams

CodeTerm teams are meta-teams: their leads shape the agents that do the work.

- **The Orchestrator coordinates and verifies at speed.** It moves between workers, dispatches, integrates, checks results, notices what is wrong and reorients quickly.
- **The Planner designs the tasks.** It looks at each task deeply and gives its Worker a task-fitted personality and role framing that narrows the Worker's focus, the way "act as an orchestrator" in the first words shapes a model.
- **The task carries the personality**: its executor role, its role guidance, and a one-word name given once at spawn. Choosing the right personality for the task is a core orchestration skill, and model choice follows the same logic: reasoning-heavy work goes to the stronger reasoner, precise execution to the executor model.
- **Workers feel alive**: stable names, character, and a presentation that shows them working and cooling down.
- **Prompts stay short and unambiguous** so the task-specific framing dominates; generic boilerplate that dilutes it is removed.

## Agent-agnostic

Model-agnostic, provider-agnostic. Claude, Codex, local models, custom agents — anything that runs in a terminal. CodeTerm provides the substrate and bounded host judgments; task reasoning stays with the agents. Each agent type gets its own provider for detection and integration, but the tools are universal.

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

Core stays small: one service layer over PTY, agents, plans, the message gateway, mesh, memory and the plugin host, reached by the UI and CLI clients (named in `essences/core.md`). Features — agent providers, bridges, secret backends, views — are plugins with declared capabilities. The [core essence](core.md) describes provider and plugin boundaries.

## A foundation for onboarding and the web

The Guide explains the spinal cord in the same terms as the product: useful reflexes, visible cost, unchanged behavior without an adapter and evidence before action. First-run setup introduces the Supporter only after the user is established. Future onboarding and the future web experience build on this shared host capability, so understanding and recovering the environment do not depend on sitting beside its desktop. Their detailed journeys remain open; the authority boundary does not.

## Future capabilities

- **Inbox triage** — a future direction, not yet planned: the spinal cord routes routine inputs for LLM agents to save their tokens; its detailed contract remains open.
- **Project indexing** — each machine indexes its projects into memory, queryable across the mesh.
- **Mini LLMs** inside CodeTerm for fast local classification, routing, and summarization.
- **Context-dependent recall** — knowledge surfaces by project, task, or person, not all at once.

## Design principles

1. **Bounded reflexes** — the OS supplies tools and small judgments; deterministic evidence authorizes actions and agents retain task reasoning
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

