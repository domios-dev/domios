# Domios core

What the running product is made of, stated as concepts that outlive any rewrite of the code. The [capability map](capabilities.md) groups what Domios does by vector; this essence names the parts those capabilities run on, the clients that reach them, and the one route every message takes. Together they are the elements of the project map. The map is an indirect beacon, not a blueprint: real architecture is more complex and is never reshaped to match it.

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
| client:ui | UI | One interface with two shells: the desktop app, reaching the service through the host, and the web client, reaching it over the owner's Tailnet from a phone or browser. The chat view renders an agent as a conversation inside either shell; plugin views run in sandboxed frames inside it | Shows what the service reports and never infers state of its own; clears the chat composer on send and keeps one bubble with subtle sending, queued, delivered or not-delivered state; Retry reuses the same bubble only for proven non-delivery; close dialogs list affected Team members, crons, and lanes; close refusals appear as toasts; rendered agent HTML and plugin views have no network or arbitrary file access; web access is Tailnet-only by default and the API token is the credential |
| client:cli | CLI | The spinal cord's control surface for agents and people, over the local socket or the authenticated API, and to other machines through the mesh | Its command tree is the contract the generated reference, chat cards and prompts follow; a contract change audits every consumer; the daemon never shells out to its own CLI |

Agents are users of the CLI, not clients of their own. Bridges, webhook senders, service daemons and mesh peers are not clients either: they enter through the message gateway or the mesh, where trust is decided.

The tab side panel offers Documents and Changes. Changes shows recorded edits from the tab's own loaded transcript, cumulatively or per turn, and follows file renames. Counts sum recorded additions and deletions; the diff history does not claim to be a net working-tree diff. Changed-file details switch between recorded Diff and the full current File, with that choice kept per tab; either view opens fullscreen. Current-file previews use the tab-scoped reader with bounded text, document, page and image rendering; deleted, moved, binary, oversized and unreadable files keep the recorded diff available. Files can be opened or revealed and their source turns reached in chat. Missing text diffs, binary files, truncation and recorded deletion are explained. Shell-made changes are outside this view's recorded coverage. Both views share one panel with a list-to-detail navigation kept per tab, where Escape or Backspace returns to the list. A document, selected document text, a changed file or a single hunk can be added to the tab's composer as attachment chips or quoted references; adding never sends.

The CLI command groups and their accepted inputs and behavior form a product contract. Its generated reference, chat actions, and agent prompts stay aligned with the command tree. Breaking CLI-to-service requests are rejected with a clear version-mismatch response and recovery guidance; additive optional operations and fields remain compatible. Command feedback distinguishes errors that require action, advisory hints, and nonfatal warnings.

## Core parts

### Service layer {#core:service-layer}

The hub. One service owns every operation on workspaces, tabs, agents and plans, and every transport delegates to it: desktop host calls, the authenticated API, the local socket and mesh relays. Validation, events and cleanup happen once, here, whatever the entry point.

- Root: true.
- Invariants: production listens on 7685 and is never killed by development, which uses 7686 or a named instance with its own data; workspace close follows the [Team teardown contract](orchestration.md#team-teardown) before removing remaining tabs; a headerless REST close is Unattributed unless its body confirms the exact target; an outdated or versionless protocol is refused; service requests stay responsive while blocking and memory-intensive work runs; background work is bounded and isolated so it cannot starve or take down the API; coordination waits on observed events, never on sleeps; utility shell lifetimes follow the [orchestration contract](orchestration.md), and declared deadlines survive restart or are reported lost; subsystems stop and start without restarting the daemon where they can.
- Dependent capabilities: workspaces and tabs, health and diagnostics, updates, and the web UI.

### General Agent {#core:general-agent}

Main is the protected, always-on agent singleton for one installation. It owns the General workspace identity, represents the machine in the mesh, receives bridge and mesh traffic, and coordinates project agents. The General workspace also hosts independent General-role sessions created by the owner.

- Part of: core:agents-and-providers.
- Invariants: exactly one protected General workspace and Main tab exist; routing reaches Main through its own tab; its provider session is resumed only from explicit ownership evidence; independent General sessions use the daemon's General home, configured provider and model policy, and permanent prompt, with their own tab, provider session and launch provenance; their launch preserves Main's aliases, selection, binding and durable session state; General identity stays confined to sanctioned sessions in General; the permanent prompt is regenerated from installation and user identity, personality, shared agent guidance, and current memory when available; unavailable memory does not block startup; relevant capability hints are optional, bounded and limited to the first seven days from a durable first-session time that never resets; first-run bootstrap belongs to Main and is not sent on session resume; the [Supporter contract](supporter-agent.md#bounded-recovery-and-unattended-work) owns bounded Main recovery and its deterministic crash exception; independent-session exits cannot trigger Main recovery.
- Dependent capabilities: workspaces and tabs, agent providers, session continuity, Telegram bridge, and remote work.

### Crash-recovery supervisor {#core:crash-recovery-supervisor}

The external supervisor observes daemon epochs and owns crash-recovery launches; the daemon never restarts itself. The supervisor adopts a matching live process instead of starting a duplicate, and it distinguishes unexpected exits from clean exits and requested handoffs. Crash classification describes the dead process: a short-lived startup failure differs from other unclean exits, and a kill or power-loss attribution requires an external observer that saw the process die. Downtime alone does not establish a cause.

- Part of: core:host-state.
- Invariants: durable recovery state precedes restart decisions and survives supervisor interruption; clean exits and requested update or restart handoffs are not treated as crashes; a later launch restores session snapshots and task bindings before any worker spawn; existing execution bindings remain authoritative; work resumes only when the same provider session and task attempt prove that safe continuation will not duplicate side effects; a missing exact binding requires review or reassignment, never a guess based on downtime; restored owner and Planner bindings keep report routing stable, and duplicate worker or report ownership is checked before recovery dispatch; the supervisor restarts the app but never resumes workers or creates duplicate app instances. The accessible recovery status remains active until restored session identities prove health, not until dismissal; resolved crash records remain available, and a later process epoch supersedes the prior warning.
- Dependent capabilities: session continuity, health and diagnostics, and updates and local install.

### Host state {#core:host-state}

What one Domios instance remembers across restarts: configuration, the workspace and tab snapshot with its lifecycle journal, schedules, the secret store, the usage ledger and the kept notification records.

- Part of: core:service-layer.
- Invariants: each instance keeps its own data directory, so development never reads or writes production's state; after storage cutover, `state.db` is the sole restore source and legacy JSON is import input only; restoring this state after a restart is unconditional and the journal is telemetry, not authority; writes to held configuration fail without changing live settings until the file is repaired or restored; secrets arrive on stdin, stay on their machine and never appear in a configuration dump.
- Dependent capabilities: workspaces and tabs, session continuity, scheduling, service registry, secret store, configuration, usage and Analytics.

A missing or unreadable workspace folder, including a retired drive or volume, is checked at startup, workspace open and spawn refusal without polling. The sidebar marks the workspace and its view offers Choose folder instead of session creation. Relocation validates the new folder and keeps workspace, tab, Team, Plan and session identities; paths beneath the old root are rebased for bound Plans, idle tabs and stored launches, while active or outside-root paths are reported unchanged. Launches refuse an unusable cwd with a human recovery message and never fall back to another folder.

### PTY {#core:pty}

Real terminals, one per tab, and the input path into them.

- Part of: core:service-layer.
- Invariants: a payload is written at most once; after it reaches the input queue a failure may only be submit unconfirmed, never unavailable; submit recovery sends a bounded blind key-only burst without screen, composer, status, or provider-identity gating; each terminal has one resettable burst and a new send cancels the old one; only an exact user turn appended after the send to the tab's own bound session confirms delivery, and each turn confirms at most one send; screen output, pre-send transcript history, and another session never prove delivery. Screen or text classification, whether heuristic or model-based, never authorizes refusing text or keys because the screen resembles a provider dialog. User sends are held only before a launch's first readiness and then flushed in order; once the agent has been ready, every send is written immediately, even while it is busy, because the provider owns its own input queue. A send's status follows reality: held, written, then delivered on an exact user turn in the tab's own bound session, otherwise submit unconfirmed; a written payload never replays after reopen or rebind. A structured runtime confirms each message only from live send evidence with that message's exclusive send id on the tab's bound thread, nonce and runtime epoch. Before thread binding the send stays held; accepted RPC writes stay written until exact evidence arrives, and transport uncertainty never falls back to paste. Each user message reaches the provider immediately: an idle model starts a turn, and a working model receives one steer per message without local buffering or joining. An explicit refusal because the expected turn ended starts that same message once; accepted or uncertain input never replays. Stop follows the provider's handling of pending input; discarded or uncertain input is never restored or replayed. Routed agent messages always queue a new turn. A programmatic send may fail only on a proven pre-write refusal. Chat reflects delivery evidence on one bubble per send. Sending clears the composer immediately; held input shows queued until the agent is ready, written or unconfirmed input shows sending, delivered input shows the normal bubble, and proven non-delivery shows a quiet mark with Retry only. Retry reuses the bubble and never replays written or uncertain input. Transcript echoes reconcile without a duplicate, repeated pending send presses cannot duplicate a bubble, reduced motion is respected, and a daemon restart never fabricates a delivery result. Launch input goes directly to the terminal already owned by its lifecycle, outside the post-ready send path. Any change to PTY or Chat input delivery gets heightened attention: it names the causal finding, the files and symbols touched and the rollback boundary, and lands only with focused proof that these invariants still hold.
- Dependent capabilities: message delivery.

Shell tabs select native or WSL profiles through the same tab contract and retain the selected profile through restore. WSL cwd changes remain visible as host drive or distribution paths. Choosing no profile preserves the existing OS default; agent launches run natively.

### Agents and providers {#core:agents-and-providers}

What makes a tab an agent: provider capabilities, launch and resume, detection, session binding and the chat-backend shells that run without a terminal. A plugin-backed shell's active system prompt is visible in chat. The status line separates the harness identity from its model control, whose complete-catalogue access follows the [agent model contract](capabilities.md#agent-models).

- Part of: core:service-layer.
- Invariants: each provider declares its transport, behavior, models, reasoning efforts, and presets; core asks capabilities, never provider identities or display names; a provider may declare a safe onboarding choice for delegated Worker and Planner launches, while owner-facing prompts stay visible to the owner; a session is bound only from the provider's own report or an explicit launch or resume choice, never inferred; daemon restore honors provider-scoped `resume_sessions` (default true), and a disabled setting fresh-launches a known provider while keeping its previous binding in history; failed launches preserve provider, role, profile, saved session and exact restore data across restarts; agent restart resumes a session with this tab's own verified binding or journaled launch target only after its provider confirms existence for the tab's cwd, then binds a new nonce; saved recovery sessions never silently become fresh conversations; `agent restart` authenticates the recovered session under its new launch nonce, and fresh-launches a known provider from a confirmed `LaunchBindingUnconfirmed` shell when no verified recovery session is available; its result reports `resumed` or `fresh_launch` and never succeeds without an observed ready agent; model, status, usage, and transcript reads use only the tab's own bound session, and unknown stays unknown; detection uses structural signals, never vendor wording alone; native and plugin providers honor the same capability contract. The [permission contract](permissions.md) owns agent access and folder trust. Provider lifecycle owns update coordination; the Supporter supplies classification, not update authority.

An exact restored session is never silently replaced. When the provider confirms it is missing, the owner can choose Start new session on the failure card or explicitly request `agent restart --fresh`. That action keeps the tab identity, cwd, Team, plan binding, model and effort; an existing exact session refuses replacement, and a live agent requires force.

#### Structured thread control

The `agentRuntime` capability selects a Chat-only stdio runtime. Each tab owns its process, exact thread and binding; core owns process lifetime, framing, delivery and presentation, while the provider plugin owns protocol methods, parameters and event interpretation. The transcript has one producer with stable item IDs and paginated history; live items and backfill reconcile without duplicates. Approval and question answers target only a pending request from the exact current binding and runtime epoch. Unsupported terminal keys and resize are explicit refusals.

Exit or pipe failure unloads that runtime epoch and keeps an honest visible state with a reason and Retry. A provider-ready event, send, command or Retry can initialize and resume the same persisted thread within a bounded failure deadline. Recovery has no timer loop and cannot silently open a different conversation or switch to a terminal. History reconciles display, never authorizes replay or confirms a new send from an old matching row. A queued payload or claimed launch brief never replays after uncertainty. Stop interrupts the active turn without destroying the runtime; close, provider change and explicit shutdown end its scope. These invariants hold for General, project, mesh-remote and restored tabs across app restarts and provider updates.

A NextTurn model, effort or runtime-mode request is pending until thread-owned settings or model evidence confirms it. It never restarts the tab. The next turn carries those controls without changing machine-global defaults. Completed compaction items restore the developer contract and cached client context; duplicate notifications do not inject twice.

Providers may declare live model and effort commands. These run only at an idle prompt in the same verified session. Confirmation comes from fresh output or a changed provider-declared identity record in that session; a timeout reports failure and never restarts automatically. Restart and resume remains an explicit fallback. The provider owns whether its command also saves a default for future sessions.

#### Provider lifecycle

Provider CLI updates are silent host operations using the provider's declared updater and compatibility verdict. Production participates by default; other instances opt in. A default-on setting allows opting out. An active provider turn defers the update to an idle event. Running agents are never restarted to apply an update; subsequent launches use the updated CLI.

A classified update prompt receives only the provider-declared safe option. A declared-incompatible target is refused. Results and captured failures go to logs and the provider's Settings status, without a bell entry or an owner action prompt. The host retains durable update claims and process ownership evidence to prevent concurrent or replayed operations.

### Prompts {#core:prompts}

The words an agent starts from: shared guidance, role contracts for Orchestrator, Planner and Worker, the task's own framing, the General Agent's permanent prompt and first-run bootstrap, and the memory worker prompts.

- Part of: core:agents-and-providers.
- Invariants: prompts owned by the product ship with the app and are composed for their role rather than copied between roles; the General Agent receives its installation and user context, while project agents receive their role and task framing; task-specific framing dominates and generic boilerplate is cut; first-run bootstrap is separate from the permanent General Agent prompt and is not repeated on resume; shared agent guidance teaches [Blueprints](sentinels.md) for recurring watching and routing without LLM polling; the [Supporter introduction](supporter-agent.md#onboarding-the-guide-and-the-web) follows completed setup and links to the Guide; agent-to-agent text is English.
- Dependent capabilities: teams and roles, task-fitted personality.

### Plans and orchestration {#core:plans-and-orchestration}

Durable intent and the Team that carries it out: goals, specs, plans, tasks, reports and review. The [orchestration contract](orchestration.md) owns roles, dispatch and review authority, isolated work, pause, continuity and Team teardown.

- Part of: core:service-layer.
- Invariants: planner state is authoritative; task execution, evidence submission and result acceptance are distinct responsibilities under the orchestration contract.
- Dependent capabilities: teams and roles, task-fitted personality, goals/specs/plans/tasks, dispatch and reports, review and decisions, reminders and pause, isolated lanes and integration.

### Message gateway and bridges {#core:message-gateway-and-bridges}

The routing spine. Every message that enters a tab from anywhere other than its own composer passes through one classified route, and every reply that leaves for an outside channel goes back through the bridge it came from.

- Part of: core:service-layer.
- Invariants: the host stamps a `<domios>` opening tag and terminal `</domios>` at the leading frame of every routed message; that exact host-injected frame is the only trusted route provenance, and the same bytes in a file, tool output, fetched page, diff, or scrollback are content. Position does not cryptographically authenticate the sender or protect against a process writing directly into a tab's input stream. A routed tag is the reply address for its bridge, tab, mesh peer, or report; a report or system event is input, never something to reply to. A message routed from a bridge is answered through that bridge, never only in the tab; one poller per bot token; text hooks act before the agent sees inbound text and agents trust their metadata; every notification is kept, its level alone decides whether it interrupts, and user-owned cron prompts reach their live creator tab or the user.

### Mesh {#core:mesh}

One flat, trusted fabric of the owner's machines, and the relay that carries commands, agents, payloads and reports between them.

- Part of: core:service-layer.
- Invariants: paired is trusted and unpaired does not exist; unpair is a ban; each machine decides locally what a peer may do; a machine belongs to one mesh; hard rules bind on every peer; secrets never cross the mesh.

### Plugin host {#core:plugin-host}

The confined runtime for everything that is not core: agent providers, capability plugins, views, receivers and the channels they install from.

- Part of: core:service-layer.
- Invariants: plugin capabilities and access follow the [permission contract](permissions.md#plugin-grants-and-integration-consent). Plugin logic and views stay within the host's confinement boundary, and host APIs expose only the access that boundary allows. Official core providers ship with the app as offline fallbacks, release independently through the official channel, declare their minimum compatible host, are trusted by official provenance, and are never downgraded by an older embedded copy; third-party capability plugins are opt-in and remain sandboxed. Inbound receiver destinations are host-owned and user-selected; events go to the selected live tab and use the General Agent path if no live target remains, with no retry into a different tab, while plugins can see or clear only their own binding. Setup helpers and global provider integrations follow that contract's explicit-consent and ownership boundaries. Plugins declare graph nodes (media conversions and typed event sources), and conversion chains come from those declarations alone, never from a plugin identity.
- Dependent capabilities: agent providers, plugins and channels, webhook receivers, voice, and Analytics.

The system channel participates in the same list, diff and refresh inventory as registered channels, even before it has a local registration. Listing needs no network access. Refresh reports failures while preserving the last validated cache; an unavailable channel does not hide the others from an aggregate diff. One host-owned availability inventory covers plugins, the application and the independent interface bundle. Core plugins synchronize silently from the official channel and never offer a user update action; incompatible offers name the host requirement. Completed actions publish the changed inventory. One grouped Update surface counts compatible newer user offers, carries Update all intent through an app-first restart, and rechecks plugin compatibility against the resumed host before offering the Interface swap. Interface swaps require explicit consent, health confirmation and rollback on failure.

### Supporter Agent {#core:supporter-agent}

The judgment part of Domios's spinal cord: small model decisions shared by memory, tab observation and the host's bounded reflexes. It has no Team role or task authority. The [Supporter Agent essence](supporter-agent.md) is the authoritative home for connection and fallback, model-first event classification and heuristic fallback, recovery, request accounting and optional caps, fan-out, spend, Blueprint decisions, visibility, data handling and future onboarding and web-client use. The marketing website consumes these contracts to explain the product.

- Part of: core:service-layer.
- Invariants: the model names the state and deterministic evidence authorizes the action; the host contract stays independent of any adapter or provider.
- Dependent capabilities: memory decision recall, health and diagnostics, session continuity, scheduling, Blueprints, provider updates, guided onboarding and the web UI.

### Memory {#core:memory}

Durable knowledge for every agent on the machine: facts, notes, preferences and skills, with retrieval and reversible consolidation. The [memory contract](memory.md) owns storage boundaries, recall, candidate sources, access accounting and hygiene.

- Part of: core:service-layer.
- Invariants: knowledge and conversation history remain distinct; retrieval and consolidation follow the authoritative memory contract.
- Dependent capabilities: memory and skills, memory decision recall, and memory hygiene.

## The routing spine: where messages and reports flow

1. **A person at the tab.** A chat turn or keystroke goes from the UI through the service layer to the capability-selected transport and is delivered at most once.
2. **A person away from the machine.** Telegram text arrives at its bridge, passes text hooks and voice transcription when needed, and enters the General Agent's tab or the tab attached to its topic with a `<domios>` route tag. The agent answers with a bridge send, and the bridge carries it back.
3. **A daemon or webhook.** A registered service pushes with its own token and a plugin receiver accepts an event; both enter the gateway and land in the chosen tab.
4. **Another agent.** A send from one tab to another is stamped with the sender's `<domios>` tag, so the recipient replies by command rather than in its own output.
5. **Another machine.** A mesh peer's message arrives through the relay with a mesh `<domios>` tag, and the reply goes back to that peer and tab.
6. **A Worker's report.** The report command records the handoff on the task as a stride, moves the task to review and pushes a report-marked message into the Orchestrator's tab, across the mesh when the Worker runs elsewhere. The [orchestration contract](orchestration.md#dispatch-reports-and-review) owns the report and review authority.
7. **The host itself.** Plan reminders, scheduled sends and system events are pushed into the tab through the same gateway, so no agent polls for them.
8. **Out to the owner.** An agent notification is kept in the panel; its level decides whether it also reaches the desktop, sound or a push channel carried by a bridge. A question to the owner offers choices, and the chosen label returns to the tab that asked.

## Leftovers, and why they stay out of the core

- **The repository toolchain.** Canonical builds, tests, release workflows and the build gateway serve the project, not the running product. Release and canonical verification point to it instead of to a core part.
- **External engines.** Speech recognition, provider CLIs and model APIs sit behind contracts the core owns; the core never absorbs them.
- **Views without authority.** Mission Control, the plan canvas, Analytics and the Mesh tab are presentations inside the UI client; the authority behind them stays in plans, host state and the mesh.

Tool-less providers declare `toolLess` as a class capability. Agent-created tabs in this class are managed by their creator: users can observe and stop a generation through the provider stop contract, while agent tabs and utility shells in the manager's Team can send work. Closing the manager returns input ownership to the user. User-created tabs remain user-owned.

Reply relay additionally requires `replyRelay` and authoritative completion evidence from the exact bound session. Routed work, including a spawn brief, receives one final answer or explicit failure notice. History and completion sidecar events drive observation; missing activation evidence yields an unavailable notice. Replies carry bounded hops and use the ordinary routed send path. Session identity, launch generation, original user boundary and payload must match before any answer is relayed.

Task-bound tool-less Workers use a host-owned lifecycle. An authenticated exact user-turn append acknowledges their dispatched task; screen state and elapsed time cannot ACK it. Authoritative final completion relays the answer once. Only an own-session completed assistant reply beginning with a well-formed report marker creates a task report; ordinary answers do not. The backend strips that first marker line from both the routed answer and report body. Follow-up turns follow the same rule. Failed turns propose blocked status; unavailable completion support produces an explicit coordination incident. Their generated briefs carry the task contract without CLI ACK/report instructions or command primers.

Account authentication stays behind a provider interface in the service layer. Hosted sign-in opens the system browser and completes only after a matching callback and accepted PKCE verifier. Account identity reaches Settings and the local CLI/API; only the secret store persists the refresh credential, and ID tokens stay in memory. A local provider supports development without cloud setup.
