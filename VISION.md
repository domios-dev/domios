# CodeTerm Vision

## What it is

CodeTerm is a **distributed OS for AI agents**. Not a terminal with AI bolted on — it's infrastructure that agents need to live, work, and coordinate, wrapped in a terminal because that's where developers work.

## Core metaphor: spinal cord

The CodeTerm CLI and APIs are the **spinal cord** — low-level reflexes that any agent uses to act within the environment. An agent doesn't need to understand how panes, workspaces, or mesh work internally. It just uses the tools: split a pane, read output, spawn a worker, send a message. Like a spinal cord handles reflexes so the brain can focus on thinking.

The **General Agent** is the brain of one machine — manages the environment, routes messages, delegates work. But it's just one agent among many. Other agents get the spinal cord (the `/codeterm` skill) but NOT the General Agent's identity or personality. They know how to act inside CodeTerm without becoming the General Agent.

## Agent-agnostic

Model-agnostic, provider-agnostic. Claude, Codex, Gemini, local models, custom agents — anything that runs in a terminal. CodeTerm provides the substrate, not the intelligence. Each agent type gets its own provider for detection and integration, but the tools are universal.

## Isolation principle

Each agent has its own scope:
- **General Agent** — machine-level brain, lives in `~/.codeterm/general/`, writes its own CLAUDE.md there
- **Project agents** — live in project workspaces, get the `/codeterm` skill automatically, know how to use the tools but don't inherit General Agent's role or knowledge
- **Spawned workers** — ephemeral, task-scoped, report and die

An agent's identity comes from its task and workspace, not from CodeTerm. CodeTerm gives them hands and eyes, not a personality.

## Mesh: distributed fabric

Today: machines discover each other on LAN, pair with trust verification, relay commands via `--mesh`. Crown leadership for coordination.

Tomorrow: mesh becomes a **unified fabric**. All CodeTerm instances across all machines form one distributed environment where:
- **Shared memory** — agents across machines share knowledge, context, and recall. Possibly replicated on all nodes, with crown holding authoritative state.
- **Project awareness** — each machine indexes its projects. Any agent can query "where is the backend repo?" and get routed to the right machine.
- **Distributed apps** — internal tools that replicate across the mesh (dashboards, monitors, shared notebooks).
- **Unified delegation** — "build this" finds the best machine (has GPU, has the repo, is idle) without the user specifying which peer.

## Future capabilities

Things we'll likely build:
- **Vector DB / embeddings** for project indexing and semantic recall across workspaces
- **Mini LLMs** running inside CodeTerm for fast local tasks (classification, routing, summarization) without API calls
- **Context-dependent recall** — the system surfaces relevant knowledge based on what project/task/person you're working with, not all at once
- **Terminal as API** — agents spawn raw terminals, execute commands, read output, without needing a human shell. CodeTerm becomes the bridge between agents and the real OS.
- **Replicating knowledge base** — mesh peers sync relevant knowledge (project state, agent learnings, user preferences) so any machine has what it needs

## Design principles

1. **Spinal cord, not brain** — CodeTerm provides reflexes (tools), not intelligence (models)
2. **Agent-agnostic** — never hardcode for one provider
3. **Isolation** — agents don't leak into each other's identity or scope
4. **Distributed-first** — every feature should work across mesh, not just locally
5. **Terminal-native** — the terminal is the universal interface; don't fight it, enhance it
6. **Progressive capability** — works standalone, gets better with mesh, gets smarter with memory/indexing
