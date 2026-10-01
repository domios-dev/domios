# CodeTerm core

What the running product is made of, stated as concepts that outlive any rewrite of the code. The [capability map](capabilities.md) groups what CodeTerm does by vector; this essence names the parts those capabilities run on, the clients that reach them, and the one route every message takes. Together they are the elements of the project map. The map is an indirect beacon, not a blueprint: real architecture is more complex and is never reshaped to match it.

Every element carries an explicit stable id in its metadata: a core part heading ends with `{#core:<slug>}` and a client row's first cell is its id. The project's core root is the one part that declares `- Root: true`; every other part declares `- Part of: <id>`. The root is authored, so any project topology is representable, including a library or offline tool with no clients.

## How to read the links

- A capability **runs on** the core parts or clients that carry it.
- A capability **feeds** another when its output becomes the other's input: work, messages, evidence or state. Every capability has at least one feeds link in or out, or the map records why not.
- A core part is **part of** the core root or of another core part.
- A client is a **client of** the core root.
- The budget is about one hundred nodes per project: vectors, capabilities, core parts and clients. Links do not count against it. When the map grows past it, essences are merged or simplified; the architecture is not.

## Clients are not the core

A client presents and requests. It holds no authority of its own: every state it shows comes from the service layer, and every change it makes is a request the service layer may refuse.

| ID | Client | What it is | Invariants |
|---|---|---|---|
| client:ui | UI | One interface with two shells: the desktop app, reaching the service through the host, and the web client, reaching it over the owner's Tailnet from a phone or browser. The chat view renders an agent as a conversation inside either shell; plugin views run in sandboxed frames inside it | Shows what the service reports and never infers state of its own; renders no delivery banners, failure cards or retry actions; close dialogs list affected Team members, crons, and lanes; close refusals appear as toasts; rendered agent HTML and plugin views have no network or arbitrary file access; web access is Tailnet-only by default and the API token is the credential |
| client:cli | CLI | The spinal cord: the one control surface for agents and people, over the local socket or the authenticated API, and to other machines through the mesh | Its command tree is the contract the generated reference, chat cards and prompts follow; a contract change audits every consumer; the daemon never shells out to its own CLI |

Agents are users of the CLI, not clients of their own. Bridges, webhook senders, service daemons and mesh peers are not clients either: they enter through the message gateway or the mesh, where trust is decided.

The CLI command groups and their accepted inputs and behavior form a product contract. Its generated reference, chat actions, and agent prompts stay aligned with the command tree. Breaking CLI-to-service requests are rejected with a clear version-mismatch response and recovery guidance; additive optional operations and fields remain compatible. Command feedback distinguishes errors that require action, advisory hints, and nonfatal warnings.

## Core parts

### Service layer {#core:service-layer}

The hub. One service owns every operation on workspaces, tabs, agents and plans, and every transport delegates to it: desktop host calls, the authenticated API, the local socket and mesh relays. Validation, events and cleanup happen once, here, whatever the entry point.

- Root: true.
- Invariants: production listens on 7685 and is never killed by development, which uses 7686 or a named instance with its own data; a confirmed human may close a workspace containing a coordinator; an agent cannot close its own coordinator workspace; a headerless REST close is Unattributed unless its body confirms the exact target; workspace close sends each Team through Team close before removing remaining workspace tabs; an outdated or versionless protocol is refused; service requests stay responsive while blocking and memory-intensive work runs; background work is bounded and isolated so it cannot starve or take down the API; coordination waits on observed events, never on sleeps; subsystems stop and start without restarting the daemon where they can.
- Dependent capabilities: workspaces and tabs, health and diagnostics, updates, and the web UI.

### General Agent {#core:general-agent}

The protected, always-on agent singleton for one installation. It owns the General workspace identity, represents the machine in the mesh, receives bridge and mesh traffic, and coordinates project agents.

- Part of: core:agents-and-providers.
- Invariants: exactly one protected General workspace and tab exist; routing reaches the singleton through its own tab; its provider session is resumed only from explicit ownership evidence; its permanent prompt is regenerated from installation and user identity, personality, shared agent guidance, and current memory when available; unavailable memory does not block startup; first-run bootstrap is a separate one-time message and is not sent on session resume.
- Dependent capabilities: workspaces and tabs, agent providers, session continuity, Telegram bridge, and remote work.

### Crash-recovery supervisor {#core:crash-recovery-supervisor}

The external supervisor observes daemon epochs and owns crash-recovery launches; the daemon never restarts itself. The supervisor adopts a matching live process instead of starting a duplicate, and it distinguishes unexpected exits from clean exits and requested handoffs. Crash classification describes the dead process: a short-lived startup failure differs from other unclean exits, and a kill or power-loss attribution requires an external observer that saw the process die. Downtime alone does not establish a cause.

- Part of: core:host-state.
- Invariants: durable recovery state precedes restart decisions and survives supervisor interruption; clean exits and requested update or restart handoffs are not treated as crashes; a later launch restores session snapshots and task bindings before any worker spawn; existing execution bindings remain authoritative; work resumes only when the same provider session and task attempt prove that safe continuation will not duplicate side effects; a missing exact binding requires review or reassignment, never a guess based on downtime; restored owner and Planner bindings keep report routing stable, and duplicate worker or report ownership is checked before recovery dispatch; the supervisor restarts the app but never resumes workers or creates duplicate app instances. The accessible recovery status remains active until restored session identities prove health, not until dismissal; resolved crash records remain available, and a later process epoch supersedes the prior warning.
- Dependent capabilities: session continuity, health and diagnostics, and updates and local install.

### Host state {#core:host-state}

What one CodeTerm instance remembers across restarts: configuration, the workspace and tab snapshot with its lifecycle journal, schedules, the secret store, the usage ledger and the kept notification records.

- Part of: core:service-layer.
- Invariants: each instance keeps its own data directory, so development never reads or writes production's state; restoring this state after a restart is unconditional and the journal is telemetry, not authority; writes to held configuration fail without changing live settings until the file is repaired or restored; secrets arrive on stdin, stay on their machine and never appear in a configuration dump.
- Dependent capabilities: workspaces and tabs, session continuity, scheduling, service registry, secret store, configuration, usage and Analytics.

### PTY {#core:pty}

Real terminals, one per tab, and the input path into them.

- Part of: core:service-layer.
- Invariants: a payload is written at most once; after it reaches the input queue a failure may only be submit unconfirmed, never unavailable; submit recovery sends a bounded blind key-only burst without screen, composer, status, or provider-identity gating; each terminal has one resettable burst and a new send cancels the old one; only an exact user turn appended after the send to the tab's own bound session confirms delivery, and each turn confirms at most one send; screen output, pre-send transcript history, and another session never prove delivery. User-initiated uncertainty remains pending rather than becoming a refusal, while a programmatic send may fail only on a proven pre-write refusal. The delivery ledger is internal telemetry: Chat UI renders no delivery banners, failure cards, Retry actions, or notifications, and a daemon restart never fabricates a delivery result. Launch input goes directly to the terminal already owned by its lifecycle, outside the post-ready send path. Any change to PTY or Chat input delivery needs the owner's explicit approval first.
- Dependent capabilities: message delivery.

### Agents and providers {#core:agents-and-providers}

What makes a tab an agent: provider capabilities, launch and resume, detection, session binding and the chat-backend shells that run without a terminal. A plugin-backed shell's active system prompt is visible in chat.

- Part of: core:service-layer.
- Invariants: each provider declares its transport, behavior, models, reasoning efforts, and presets; core asks capabilities, never provider identities or display names; a provider may declare a safe onboarding choice for delegated Worker and Planner launches, while owner-facing prompts stay visible to the owner; a session is bound only from the provider's own report or an explicit launch or resume choice, never inferred; model, status, usage, and transcript reads use only the tab's own bound session, and unknown stays unknown; detection uses structural signals, never vendor wording alone; native and plugin providers honor the same capability contract.

### Prompts {#core:prompts}

The words an agent starts from: shared guidance, role contracts for Orchestrator, Planner and Worker, the task's own framing, the General Agent's permanent prompt and first-run bootstrap, and the memory worker prompts.

- Part of: core:agents-and-providers.
- Invariants: prompts owned by the product ship with the app and are composed for their role rather than copied between roles; the General Agent receives its installation and user context, while project agents receive their role and task framing; task-specific framing dominates and generic boilerplate is cut; first-run bootstrap is separate from the permanent General Agent prompt and is not repeated on resume; agent-to-agent text is English.
- Dependent capabilities: teams and roles, task-fitted personality.

### Plans and orchestration {#core:plans-and-orchestration}

Durable intent and the team that carries it out: goals, specs, plans, tasks and strides; the Orchestrator, optional Planner, optional Cartographer Manager, and Workers; dispatch, acknowledgement, reports, review and lanes.

- Part of: core:service-layer.
- Invariants: planner state is the source of truth; the Planner shapes tasks, or the Orchestrator does when planning is waived; a Team-adopted plan binds its unique live Planner; Team close inventories every Team-owned task lane and reports removed, retained-with-reason, and worktree-less branch entries; it removes only clean lanes whose branch tip is an ancestor of the recorded destination and records unsafe lanes as cleanup debt; it finalizes open attempts and Team crons; it refuses while a Team task is in progress or a Worker is working unless forced, a forced close names every killed Worker and task, and a plan with unfinished tasks stays active and flagged rather than archived; a Worker that dies holding an in-progress task is pushed to its Orchestrator at once as one structured event naming task, lane, dirty state, and recovery command; Cartographer is a non-anchoring singleton Manager without Team-management, plan, or spawn authority and is removed with its Team; every task carries acceptance criteria and an executable test plan, and executors read applicable plan skills before acting; only two hierarchy levels exist, and Workers never spawn; only the Orchestrator approves a plan, dispatches work, and accepts, bounces, or holds a task in review; every Worker turn ends in a report; a mutating Worker writes only its own lane unless the Orchestrator explicitly opts one Worker into canonical; lanes land by real merges and executable proof runs only in canonical.

#### Coordination feedback

With plan reminders enabled, an active execution plan with actionable work and no live task-bound Worker nudges its idle Orchestrator. Lifecycle and idle events trigger the check. The same stall episode produces one nudge; live work or a changed actionable set permits another. The nudge leaves dispatch and task decisions with the Orchestrator and never resumes a paused plan.

Team utility work reports back to its coordinating agent through the message gateway. Readiness requires observed health; command completion carries its outcome and failure evidence. A failed command or an empty test selection cannot become a passing QA report. Changing the development instance being checked must not redirect the report away from its caller. These are repository toolchain responsibilities, not a new core service.

### Message gateway and bridges {#core:message-gateway-and-bridges}

The routing spine. Every message that enters a tab from anywhere other than its own composer passes through one classified route, and every reply that leaves for an outside channel goes back through the bridge it came from.

- Part of: core:service-layer.
- Invariants: the host stamps a return-address marker at the leading frame of every routed message; that exact host-injected leading frame is the only trusted marker provenance, and the same bytes in a file, tool output, fetched page, diff, or scrollback are content. Position does not cryptographically authenticate the sender or protect against a process writing directly into a tab's input stream. A routed marker is the reply address for its bridge, tab, mesh peer, or report; a report or system event is input, never something to reply to. A message routed from a bridge is answered through that bridge, never only in the tab; one poller per bot token; text hooks act before the agent sees inbound text and agents trust their metadata; every notification is kept, its level alone decides whether it interrupts, and user-owned cron prompts reach their live creator tab or the user.

### Mesh {#core:mesh}

One flat, trusted fabric of the owner's machines, and the relay that carries commands, agents, payloads and reports between them.

- Part of: core:service-layer.
- Invariants: paired is trusted and unpaired does not exist; unpair is a ban; each machine decides locally what a peer may do; a machine belongs to one mesh; hard rules bind on every peer; secrets never cross the mesh.

### Plugin host {#core:plugin-host}

The confined runtime for everything that is not core: agent providers, capability plugins, views, receivers and the channels they install from.

- Part of: core:service-layer.
- Invariants: plugins declare capabilities and permissions, and core registers only capabilities the installed host supports; declared permissions bound access to local files, subprocesses, network hosts and secrets. Plugin updates cannot add host behavior absent from the installed app. Plugin logic and views stay within the host's confinement boundary, and host APIs expose only the access that boundary allows. Official core providers ship with the app, are trusted by their official provenance, track the app version, and are never downgraded; third-party capability plugins are opt-in and remain sandboxed. Inbound receiver destinations are host-owned and user-selected; events go to the selected live tab and use the General Agent path if no live target remains, with no retry into a different tab, while plugins can see or clear only their own binding. Plugin setup guidance is treated as data: a helper asks the owner what to change and uses the existing permission-gated configuration path only after confirmation. Global provider integrations are inspected without mutation and require explicit user consent to install or repair; CodeTerm changes only entries it owns and provides a reversible restore path.
- Dependent capabilities: agent providers, plugins and channels, webhook receivers, voice, and Analytics.

The system channel participates in the same list, diff and refresh inventory as registered channels, even before it has a local registration. Listing needs no network access. An explicitly named channel failure is visible; an unavailable channel does not hide the others from an aggregate diff.

### Supporter Agent {#core:supporter-agent}

A small decision model that answers bounded judgment questions for CodeTerm. Memory uses it to judge relevance; shadow tab observation uses it to classify visible state. It is a shared capability, with no Team role, task execution or input authority of its own.

- Part of: core:service-layer.
- Invariants: callers select declared choice, yes/no probability (`noul`) or score capabilities through a common registry, never branch on provider identity; each feature is independently optional and keeps its own failure boundary; model output never grants permission, proves session ownership or confirms input delivery.
- Dependent capabilities: memory decision recall, health and diagnostics.

TypeSafe Jev is the first adapter. The contract belongs to the host and stays independent of that adapter. Calls have bounded failure deadlines. Memory sends the query and candidate titles and bounded body text; tab observation sends recent screen text. Enabling one use does not enable another.

Credentials belong to the instance and adapter. They enter through CLI stdin or the write-only field in Settings > Agents > Supporter Agent, never argv, and never appear in logs or status responses. Owner-only local files (0600 on Unix) keep them outside the vault to meet the recall latency requirement; Unix permissions and Windows access controls enforce that boundary. Status exposes adapter availability, token presence and permission evidence without returning the token.

Shadow tab observation is off by default. At a startup-health check it records a model label and confidence alongside the structural verdict, including agreement, disagreement or uncertainty. It cannot change the nudge, readiness, delivery or prompt response. Failure leaves the structural path unchanged. Shadow evidence is observation, not authority to automate a recovery.

### Memory {#core:memory}

Durable knowledge for every agent on the machine: facts, notes, preferences and skills, with hybrid search, a graph and nightly consolidation.

- Part of: core:service-layer.
- Invariants: persistent memory lives here and never in provider side stores; it is English and its bodies carry no dates; skills are registered here, never written into provider skill directories; conversations are searched as sessions, durable facts as memory.

#### Decision recall

Recall can add an optional second stage that judges how directly and usefully each candidate answers the query. The first stage retains ordinary hybrid retrieval; the [Supporter Agent](#core:supporter-agent) judges candidates independently and orders the pool by absolute relevance. Each candidate receives one yes/no probability judgment when supported, otherwise a normalized score. Ties keep the original order.

- Ordinary search callers keep their existing query, filters and limit. Instance configuration enables the stage; a caller may override it for one search. It is off by default, and when off the response is byte-identical to ordinary recall.
- Relevance changes order, never eligibility: every pool candidate remains eligible for the returned list, cut only to the caller's limit. Every hit returned by an applied stage carries `relevance`. `low_confidence` flags that the maximum relevance is below the configured threshold; it never removes results.
- The second stage is all-or-nothing under one bounded failure deadline, defaulting to 3 seconds. On unavailable or failed decision scoring, the caller receives exactly ordinary recall hits for the original query and limit. An unregistered adapter id, unsupported decision capabilities, an adapter error, or expiry adds a fallback reason in the decision envelope. Missing credentials also fall back when the pool requires scoring. A partially judged pool is never returned.
- Gathering and judging candidates record no access. Only memory hits actually returned record access, once per item, using ordinary memory access accounting. Additional candidates never inflate usage or evidence merely by entering the pool.
- Memory > Settings controls recall; the memory search CLI and API expose whether the stage applied, fell back or was skipped. Shared credentials and adapter status belong to the Supporter Agent. Explicitly requesting both decision recall and legacy reranking is refused; legacy reranking requested while decision recall is enabled only by configuration runs with a skipped reason.

#### Optional candidate sources

The ordinary recall pool defaults to 30 candidates and expands when needed for the caller's limit, within the retrieval cap. Optional sources can supply additional candidates to the same decision pool. Each source is independently configurable, respects the search's applicable filters and contributes results only when the decision stage is applied. Setting its candidate count to zero disables it. Its additions never appear in fallback. A failing optional source is skipped and never fails the search; decision-model errors follow the all-or-nothing fallback rule above. Gathering candidates never changes stored memory knowledge or access evidence; returned memory hits follow the accounting rule above.

- Graph (default 3): adds memory items connected to query-relevant entities that ordinary recall did not already place in the pool.
- Docs (default 3): adds relevant sections of embedded product help and leaf command sections of the generated CLI reference (`CLI`), identified by document and section, with a pointer to the document or command help. It never reads or changes memory state.
- Sessions (default off): adds matching excerpts of past provider conversations with their session provenance. Source failure or expiry skips these candidates; their presence does not turn a conversation into stored memory.

Source admission policy: the owner admits and retains sources only when measurements show better answers while keeping recall fast. This policy governs source selection; enabling a source does not certify its benefit.

#### Reversible hygiene

The Dreamer consolidates memory through bounded, reviewable changes. A merge archives the lesser item, preserves its history and links it to the keeper; a conflict records which item supersedes the other. The keeper may receive a revised body in the same operation. Nothing is hard-deleted. Pinned items, skills, rules and preferences cannot be merged away.

Restoring an archived item returns its prior kind and invalidates the relation created by its merge while preserving the relation's history. Body versions remain separately restorable: restoring the lesser item does not silently undo a keeper-body revision. Dreamer merges and stale archives have separate daily caps enforced by the host.

Stale archival is off by default and has a read-only preview with item ids and reasons. An item qualifies only when it is older than a year and has had no direct access, update or history change, no relation change, and no touch through an active neighbour during that year. Pinned items, skills, rules, preferences, already archived items and high-importance knowledge are excluded. Archival records the reason and remains reversible.

The graph hub report provides bounded, deterministic cluster suggestions and existing topic anchors. Suggestions distinguish structural congestion from provenance and leave graph changes to explicit operations; the Dreamer does not have to invent a grouping from an unbounded graph dump.

Run completion and diary finalization are separate facts. If a deadline or blocking command-approval prompt ends a dreaming run, the host preserves the work already logged, marks unfinished stages as cut short and records partial finalization with its reason. It never upgrades interrupted work to success, and it preserves an already completed diary.

## The routing spine: where messages and reports flow

1. **A person at the tab.** A chat turn or keystroke goes from the UI through the service layer to the PTY and is delivered at most once.
2. **A person away from the machine.** Telegram text arrives at its bridge, passes text hooks and voice transcription when needed, and enters the General Agent's tab or the tab attached to its topic with a bridge marker. The agent answers with a bridge send, and the bridge carries it back.
3. **A daemon or webhook.** A registered service pushes with its own token and a plugin receiver accepts an event; both enter the gateway and land in the chosen tab.
4. **Another agent.** A send from one tab to another is stamped with the sender's marker, so the recipient replies by command rather than in its own output.
5. **Another machine.** A mesh peer's message arrives through the relay with a mesh marker, and the reply goes back to that peer and tab.
6. **A Worker's report.** The report command records the handoff on the task as a stride, moves the task to review and pushes a report-marked message into the Orchestrator's tab, across the mesh when the Worker runs elsewhere. The Orchestrator acts on it and never replies to it; accepting, bouncing or holding is its answer. A bounce returns work to the Worker when its continuation is available; when the Worker is gone, it reopens the task as pending and preserves its note, attempt history, and lane for redispatch.
7. **The host itself.** Plan reminders, scheduled sends and system events are pushed into the tab through the same gateway, so no agent polls for them.
8. **Out to the owner.** An agent notification is kept in the panel; its level decides whether it also reaches the desktop, sound or a push channel carried by a bridge. A question to the owner offers choices, and the chosen label returns to the tab that asked.

## Leftovers, and why they stay out of the core

- **The repository toolchain.** Canonical builds, tests, release workflows and the build gateway serve the project, not the running product. Release and canonical verification point to it instead of to a core part.
- **External engines.** Speech recognition, provider CLIs and model APIs sit behind contracts the core owns; the core never absorbs them.
- **Views without authority.** Mission Control, the plan canvas, Analytics and the Mesh tab are presentations inside the UI client; the authority behind them stays in plans, host state and the mesh.
