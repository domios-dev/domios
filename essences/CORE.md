# CodeTerm core

What the running product is made of, stated as concepts that outlive any rewrite of the code. The capability map groups what CodeTerm does by vector; this essence names the parts those capabilities run on, the clients that reach them, and the one route every message takes. Together they are the elements of the project map. The map is an indirect beacon, not a blueprint: real architecture is more complex and is never reshaped to match it.

## How to read the links

- A capability **runs on** the core parts or clients that carry it.
- A capability **feeds** another when its output becomes the other's input: work, messages, evidence or state. Every capability has at least one feeds link in or out, or the map records why not.
- A core part is **part of** the service layer or of another core part.
- A client is a **client of** the service layer.
- The budget is about one hundred nodes per project: vectors, capabilities, core parts and clients. Links do not count against it. When the map grows past it, essences are merged or simplified; the architecture is not.

## Clients are not the core

A client presents and requests. It holds no authority of its own: every state it shows comes from the service layer, and every change it makes is a request the service layer may refuse.

| Client | What it is | Invariants |
|---|---|---|
| UI | One interface with two shells: the desktop app, reaching the service through the host, and the web client, reaching it over the owner's Tailnet from a phone or browser. The chat view renders an agent as a conversation inside either shell; plugin views run in sandboxed frames inside it | Shows what the service reports and never infers state of its own; renders no delivery banners, failure cards or retry actions; rendered agent HTML and plugin views have no network or arbitrary file access; web access is Tailnet-only by default and the API token is the credential |
| CLI | The spinal cord: the one control surface for agents and people, over the local socket or the authenticated API, and to other machines through the mesh | Its command tree is the contract the generated reference, chat cards and prompts follow; a contract change audits every consumer; the daemon never shells out to its own CLI |

Agents are users of the CLI, not clients of their own. Bridges, webhook senders, service daemons and mesh peers are not clients either: they enter through the message gateway or the mesh, where trust is decided.

## Core parts

### Service layer

The hub. One service owns every operation on workspaces, tabs, agents and plans, and every transport delegates to it: desktop host calls, the authenticated API, the local socket and mesh relays. Validation, events and cleanup happen once, here, whatever the entry point.

- Part of: the running daemon itself.
- Invariants: production listens on 7685 and is never killed by development, which uses 7686 or a named instance with its own data; an outdated or versionless protocol is refused; hot paths stay cheap and bounded, and blocking work never runs on the shared async runtime; coordination waits on observed events, never on sleeps; subsystems stop and start without restarting the daemon where they can.
- Dependent capabilities: workspaces and tabs, health and diagnostics, updates, and the web UI.

### Host state

What one CodeTerm instance remembers across restarts: configuration, the workspace and tab snapshot with its lifecycle journal, schedules, the secret store, the usage ledger and the kept notification records.

- Part of: the service layer.
- Invariants: each instance keeps its own data directory, so development never reads or writes production's state; restore is unconditional and the journal is telemetry, not authority; secrets arrive on stdin, stay on their machine and never appear in a configuration dump.
- Dependent capabilities: workspaces and tabs, session continuity, scheduling, service registry, secret store, configuration, usage and Analytics.

### PTY

Real terminals, one per tab, and the input path into them.

- Part of: the service layer.
- Invariants: a payload is written at most once; after it reaches the write queue a failure may only be "submit unconfirmed"; submit recovery sends keys only, within one bounded cycle per terminal, and a new send cancels the old cycle; uncertainty is settled by the tab's own session evidence, never by guessing from the screen; any change to this path needs the owner's explicit approval first.
- Dependent capabilities: message delivery.

### Agents and providers

What makes a tab an agent: provider capabilities, launch and resume, detection, session binding and the chat-backend shells that run without a terminal.

- Part of: the service layer.
- Invariants: core asks capabilities, never provider identities; a session is bound only from the provider's own report or an explicit launch choice, never inferred; model, status and transcript are read only from the tab's own session, and unknown stays unknown; detection keys on structure, never on vendor copy; native and plugin providers agree on shared fixtures.
- Dependent capabilities: agent providers, session continuity, provider integrations, setup helpers and the team observer.

### Prompts

The words an agent starts from: the primer every spawned agent receives, the role contracts for Orchestrator, Planner and Worker, the task's own framing, the General Agent's system prompt and the memory worker prompts.

- Part of: agents and providers.
- Invariants: every prompt the product owns ships inside the app and is composed, not copied, per role; task-specific framing dominates and generic boilerplate is cut; agent-to-agent text is English; tests pin the delivery mechanism, never the wording.
- Dependent capabilities: teams and roles, task-fitted personality.

### Plans and orchestration

Durable intent and the team that carries it out: goals, specs, plans, tasks and strides; the two-level hierarchy of Orchestrator, optional Planner and Workers; dispatch, acknowledgement, reports, review and lanes.

- Part of: the service layer.
- Invariants: planner state is the source of truth; two levels only, and Workers never spawn; only the Orchestrator approves a plan and closes a task; every Worker turn ends in a report; a mutating Worker writes only its own lane, never canonical unless the Orchestrator opts one Worker in; lanes land by real merges and executable proof runs only in canonical.
- Dependent capabilities: the whole meta-team vector and the lane vector up to integration and canonical verification.

### Message gateway and bridges

The routing spine. Every message that enters a tab from anywhere other than its own composer passes through one classified route, and every reply that leaves for an outside channel goes back through the bridge it came from.

- Part of: the service layer.
- Invariants: the host stamps a return-address marker at the leading frame of every routed message, and only a leading marker is authentic; a report or system event is input, never something to reply to; a message routed from a bridge is answered through that bridge, never only in the tab; one poller per bot token; text hooks act before the agent sees inbound text and agents trust their metadata; every notification is kept, and its level alone decides whether it interrupts.
- Dependent capabilities: message delivery, dispatch and reports, reminders, scheduling, Telegram and future bridges, webhook receivers, text hooks, notifications and voice.

### Mesh

One flat, trusted fabric of the owner's machines, and the relay that carries commands, agents, payloads and reports between them.

- Part of: the service layer.
- Invariants: paired is trusted and unpaired does not exist; unpair is a ban; each machine decides locally what a peer may do; a machine belongs to one mesh; hard rules bind on every peer; secrets never cross the mesh.
- Dependent capabilities: pairing and trust, remote work, service registry and bridge migration.

### Plugin host

The confined runtime for everything that is not core: agent providers, capability plugins, views, receivers and the channels they install from.

- Part of: the service layer.
- Invariants: plugins declare capabilities and core consumes only those; plugin code runs confined, and each new host function is a confinement surface to justify; core providers track the app version, and official core installs are trusted and never downgraded.
- Dependent capabilities: agent providers, plugins and channels, webhook receivers, voice, and Analytics.

### Memory

Durable knowledge for every agent on the machine: facts, notes, preferences and skills, with hybrid search, a graph and nightly consolidation.

- Part of: the service layer.
- Invariants: persistent memory lives here and never in provider side stores; it is English and its bodies carry no dates; skills are registered here, never written into provider skill directories; conversations are searched as sessions, durable facts as memory.
- Dependent capabilities: memory and skills, and the General Agent's wake-up context.

## The routing spine: where messages and reports flow

1. **A person at the tab.** A chat turn or keystroke goes from the UI through the service layer to the PTY and is delivered at most once.
2. **A person away from the machine.** Telegram text arrives at its bridge, passes text hooks and voice transcription when needed, and enters the General Agent's tab or the tab attached to its topic with a bridge marker. The agent answers with a bridge send, and the bridge carries it back.
3. **A daemon or webhook.** A registered service pushes with its own token and a plugin receiver accepts an event; both enter the gateway and land in the chosen tab.
4. **Another agent.** A send from one tab to another is stamped with the sender's marker, so the recipient replies by command rather than in its own output.
5. **Another machine.** A mesh peer's message arrives through the relay with a mesh marker, and the reply goes back to that peer and tab.
6. **A Worker's report.** The report command records the handoff on the task as a stride, moves the task to review and pushes a report-marked message into the Orchestrator's tab, across the mesh when the Worker runs elsewhere. The Orchestrator acts on it and never replies to it; accepting, bouncing or holding is its answer, and a bounce returns the task to the same Worker.
7. **The host itself.** Plan reminders, scheduled sends and system events are pushed into the tab through the same gateway, so no agent polls for them.
8. **Out to the owner.** An agent notification is kept in the panel; its level decides whether it also reaches the desktop, sound or a push channel carried by a bridge. A question to the owner offers choices, and the chosen label returns to the tab that asked.

## Leftovers, and why they stay out of the core

- **The repository toolchain.** Canonical builds, tests, release workflows and the build gateway serve the project, not the running product. Release and canonical verification point to it instead of to a core part.
- **External engines.** Speech recognition, provider CLIs and model APIs sit behind contracts the core owns; the core never absorbs them.
- **Views without authority.** Mission Control, the plan canvas, Analytics and the Mesh tab are presentations inside the UI client; the authority behind them stays in plans, host state and the mesh.

