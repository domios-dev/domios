# CodeTerm capability map

What CodeTerm fundamentally does, grouped by the few vectors that give the system its shape. Each row names a capability, what it makes possible, where it is reached (UI surface, CLI command group, API area), the rules that must not break, its status, the core parts or clients it runs on, and the capabilities it feeds. Why these vectors exist is stated once, in the vision; the core parts, clients and the routing spine are described in the core essence; how to maintain this map is stated in the documentation contract.

Code is volatile and gets rebuilt in pieces; these essences are the product's stable vectors. They rarely change, and when they do they evolve gradually instead of being restructured, which is what lets them serve as the expected inputs and outputs a change must keep.

Status: **stable** is in the latest release; **in rollout** is merged for the next release and not yet shipped; **planned** is an agreed direction with no shipped surface; **roadmap** is where the product is heading, with its exact shape still open. Detail names an embedded document (read it with `codeterm docs <NAME>`) or an internal essence.

Runs on names core parts and clients exactly as the core essence does. Feeds names capabilities of this map; a capability with no outgoing link is a sink and is fed by at least one other row.

## Invariants that bind every vector

- Production listens on port 7685 and is never killed by development work; development uses 7686 or a named instance.
- Core asks capabilities, never provider identities.
- A routed message is authentic only when the host placed its marker at the leading frame; the same bytes anywhere else are content.
- Coordination waits on observed events, never on sleeps or polling intervals.
- Changes to PTY or Chat input delivery need the owner's explicit approval first.
- Agent-to-agent traffic, memory and documentation are English.

## 1. Agents live and work in tabs

Detail: API-TABS, TAB-LIFECYCLE, ROUTING, HTML, AGENT-CONTROL; internal essences MODELS and the provider authoring guide.

| Capability | What it enables | Entry points | Invariants | Status | Runs on | Feeds |
|---|---|---|---|---|---|---|
| Workspaces and tabs | Shells and agents grouped by project, each tab recognisable for its whole life; the General workspace hosts the always-on General Agent | Sidebar; CLI `workspace`, `tab`, `tab describe`; tab and workspace API | Listing covers every workspace unless one is named; the General workspace and its agent tab are protected; a tab's name is set once at spawn, and a task-bound tab refuses rename and records progress in its description | stable; all-workspace default listing in rollout | Service layer, Host state, General Agent | Agent providers |
| Agent providers | Claude, Codex and OpenCode run in a PTY; chat-backend plugins run as agent shells with no terminal behind them | Agent picker; CLI `agent spawn`, `agent providers`, `agent models` | Detection reads only the tab's own session; an unsupported reasoning effort refuses the launch instead of downgrading | stable; Cursor in rollout; Aider and Goose planned | Agents and providers, Plugin host, General Agent | Session continuity, Usage and Analytics |
| Message delivery | Text, keys and chat turns reach a tab; authored payloads travel as private pad files | CLI `send`, `tab respond`, `tab stop`, `pad`; `--from-pad` on messaging commands; tab send and chat-send API | The payload is written at most once; once queued, a failure is only "submit unconfirmed"; the UI shows no delivery banners or retry actions; authored bodies travel as pad files, never as inline shell text | stable | PTY, Message gateway and bridges | — (sink: fed by every inbound route) |
| Chat view | An agent read and driven as a conversation: tool cards, Mermaid, sandboxed HTML, file previews, voice and image attachments | Chat view; CLI `tab chat` | Rendered HTML has no network or arbitrary file access; cards follow the CLI command tree | stable; remote attachment handoff in rollout | UI | Message delivery |
| Session continuity | Tabs return after restart; past sessions of any provider are found by content and resumed | CLI `sessions`, `tab rebind`, `tab repair`, `agent restart` | Tab restore after a restart is unconditional; a provider resumes only a session it uniquely owns; the lifecycle journal is telemetry, not authority; conversations are searched as sessions, durable facts in memory | stable | Agents and providers, Host state, General Agent, Crash-recovery supervisor | Chat view |
| Scheduling | Agent work later or on a schedule; the machine kept awake while it runs | CLI `cron`, `delayed`, `send --delay`, `keepawake`; Mesh tab crons | A cron inherits its creator's Team unless detached; keep-awake goes through CodeTerm, never OS tools directly | stable | Host state, Message gateway and bridges | Message delivery |

## 2. Agents coordinate as meta-teams through plans

Detail: ORCHESTRATION, ORCHESTRATION-OPERATIONS, PLANNER, PLAN-EDITING, API-PLAN, PROMPTS.

| Capability | What it enables | Entry points | Invariants | Status | Runs on | Feeds |
|---|---|---|---|---|---|---|
| Teams and roles | An Orchestrator coordinates and verifies at speed; an optional Planner designs the tasks; Workers each execute one task | Plan canvas; CLI `team`, `agent spawn --role`, `orchestrator` | Only two hierarchy levels exist; Workers never spawn; only the Orchestrator approves a plan and closes a task | stable | Plans and orchestration, Prompts | Task-fitted personality, Dispatch and reports |
| Task-fitted personality | Each Worker starts from a framing that narrows its attention to its task and stays a recognisable colleague | Task executor role, role guidance, personality and model (CLI `plan task add`, `plan task update`); CLI `persona` | The Planner assigns the framing while cutting the task (the Orchestrator when planning is waived); executor role and role guidance are required and cannot be blanked; the one-word executor name is given at spawn and kept; runtime overlays add leaf kinds only and never grant spawn or Team authority | stable | Plans and orchestration, Prompts | Dispatch and reports, Workspaces and tabs |
| Goals, specs, plans and tasks | Durable intent: goal, what and why, how, bounded tasks, evidence, and the plan skills every executor reads | Plan canvas; CLI `plan`, `spec`, `task`, `effort`, `inbox`, `plan skill`; plan API | Planner state is the source of truth; every task carries acceptance criteria and an executable test plan; executors read applicable plan skills before acting | stable | Plans and orchestration, UI | Dispatch and reports |
| Dispatch and reports | One call spawns a Worker, assigns the task and delivers its brief; the Worker hands back by a report pushed into the Orchestrator's tab | CLI `plan task dispatch`, `agent ack`, `agent report` | Every Worker turn ends in a report; a report is input and is never replied to; only the Orchestrator marks a task done | stable | Plans and orchestration, Message gateway and bridges | Isolated lanes, Message delivery, Review and decisions |
| Review and decisions | Results accepted or bounced on evidence; owner questions routed as decision gates | CLI `plan review`, `plan decision`, `plan qa` | A bounce names the failing command and its evidence and returns the task to the same Worker; a pass that still needs a repair is a bounce | stable | Plans and orchestration | Integration, Dispatch and reports |
| Reminders, milestones and pause | A plan keeps moving without polling; the owner can hold it | Heartbeat; CLI `plan milestone`, `plan pause`, `plan resume` | Plan pause is the only agent-issued pause; paused tasks are system holds and there is no separate pause command for a single task | stable; task-pause removal in rollout | Plans and orchestration, Message gateway and bridges | Message delivery |
| Team observer | A small, cheap local model quietly watches a working team in the background, reads a little of each agent's recent context on a slow cadence, and surfaces simple truths: what each agent is doing, how the team is doing, who has been stuck too long; its hints help the Orchestrator and the owner see the team without reading it | Watcher role on a watcher-capable open agent shell (charter and tick policy) | Observes and suggests, never acts on a task or holds authority; costs little enough to run continuously | roadmap | Agents and providers, Plans and orchestration | Teams and roles |

## 3. Work flows through isolated lanes into canonical

Detail: LANES, TESTING, TEST-AUTHORING; internal essences BUILD, RELEASE and the CLI contributor guide.

| Capability | What it enables | Entry points | Invariants | Status | Runs on | Feeds |
|---|---|---|---|---|---|---|
| Isolated lanes | Parallel writers, each in its own worktree and lane branch | CLI `plan task dispatch`, `plan lanes` | A mutating dispatch never writes the canonical checkout or another task's lane. The Orchestrator may explicitly opt one Worker into canonical instead; that opt-in is exclusive and holds lane acceptance until its verdict. Lane Workers are source-only: no builds, tests, dev servers or gates | stable | Plans and orchestration | Integration |
| Integration | Accepted lanes land in canonical, collected per version on an integration branch | Git merge in the canonical checkout; CLI `plan lane-remove` | Real merge commits only, never cherry-pick, squash or replay; a lane is deleted only after its ancestry is proven | stable | Plans and orchestration | Canonical verification |
| Canonical verification | Executable QA after merge | Build script operations; read-only verification Worker; CLI `plan qa` | One QA gate per physical machine at a time; tests run through the repository wrapper; Playwright runs only on the owner's explicit request | stable | Plans and orchestration | Release, Review and decisions |
| Generated CLI artifacts | The CLI manifest, CLI reference and chat command paths track the command tree | CLI `cli-manifest` | Regenerated by the CLI built from the checkout being changed, never hand-edited; a contract change audits docs, prompts, skills and cards | stable | CLI | Chat view, Release |
| Release | Signed artifacts for macOS, Windows and Linux plus a public updater manifest | Release tag and workflows; release skill | Artifacts come from the tagged master commit; published tags never move; released master merges back into develop; public issues close only after the released bytes are verified as served | stable | — (the repository toolchain, outside the product; see the core essence's leftovers) | Updates and local install |

## 4. Machines join a mesh

Detail: MESH, MESH-SERVICES, MESH-BRIDGE; internal essence WEB_ACCESS.

| Capability | What it enables | Entry points | Invariants | Status | Runs on | Feeds |
|---|---|---|---|---|---|---|
| Pairing and trust | A flat mesh of the owner's machines | Mesh tab; CLI `mesh` | Paired is trusted and unpaired does not exist; unpair is a ban; access level is per peer and decided locally; one machine belongs to one mesh | stable | Mesh | Remote work |
| Remote work | Tabs, agents and plan commands on another machine; remote Workers report back across the mesh; payloads handed between machines by short id | `--mesh PEER` on CLI commands; CLI `paste`; mesh relay API | Hard rules bind on every peer; secrets never cross the mesh; pasted payloads are ephemeral, with a time to live | stable | Mesh, Message gateway and bridges, General Agent | Dispatch and reports, Message delivery |
| Service registry | Daemons with health probes, push tokens and an activity log; a daemon pushes text into a tab with its own token | Mesh tab; CLI `service` | A push token is shown once; rotation invalidates the old token; a peer's services are read from that peer, never gossiped | stable | Mesh, Host state | Message delivery |

## 5. People reach agents through bridges, voice and the web

Detail: INTEGRATIONS, API-INTEGRATIONS, TELEGRAM-SETUP, TEXT-HOOKS, TRANSCRIBE, NOTIFICATIONS.

| Capability | What it enables | Entry points | Invariants | Status | Runs on | Feeds |
|---|---|---|---|---|---|---|
| Telegram bridge | Conversation with the General Agent, and with individual tabs through their own topics; the poller can move to another peer without restarting either daemon | Settings Integrations; CLI `bridge send`, `bridge thread`, `bridge react`, `bridge restart`, `bridge migrate` | A bridge-routed message is answered through the bridge, never only in the tab; one poller per bot token; migration runs on the source machine only | stable; unified topic attachment in rollout; other services planned | Message gateway and bridges, Mesh, General Agent | Text hooks, Voice, Message delivery |
| Webhook receivers | Plugins accept inbound events and deliver them to a chosen tab | CLI `plugin receiver-target`; plugin webhook API | A receiver that authenticates reads its secret from the secret store | stable; host transcription of receiver audio in rollout | Message gateway and bridges, Plugin host | Voice, Message delivery |
| Text hooks | A user daemon forwards, replaces or annotates inbound bridge text before the General Agent sees it | Integrations config | Agents act on hook metadata instead of re-classifying | stable | Message gateway and bridges | Message delivery |
| Notifications | Every agent notification is kept; its level decides whether it interrupts on desktop, sound or push | Bell panel; CLI `notify` | The panel keeps every record; Workers and Planners report instead of notifying; questions to the owner use choices, never provider prompt tools | stable | Message gateway and bridges, UI | Telegram bridge |
| Voice | Inbound voice becomes text through a swappable transcription service | Transcription settings; Transcriber plugin | The speech engine stays an external service behind one transcribe contract | stable; native local dictation planned | Plugin host, Message gateway and bridges | Message delivery |
| Web UI | The full UI from a phone or browser over the owner's Tailnet | Daemon root URL; CLI `web link` | Tailnet-only by default; the API token is the credential | stable | UI, Service layer | Chat view |

## 6. The host owns memory, secrets, plugins and configuration

Detail: MEMORY, MEMORY-USAGE, MEMORY-SERVICES, SECRETS, SKILLS, CONFIG, RUNTIME-CONFIG; internal essence PLUGINS.

| Capability | What it enables | Entry points | Invariants | Status | Runs on | Feeds |
|---|---|---|---|---|---|---|
| Memory and skills | Durable facts, notes, preferences and folder skills with hybrid search, a graph and nightly consolidation, discoverable by every agent | Memory UI; CLI `mem`, `mem skill`; memory API | English only; persistent memory lives here and never in provider side stores; bodies carry no dates; skills are registered through memory, never written into provider skill directories | stable | Memory | Agent providers |
| Secret store | Local credentials for CodeTerm, plugins and bridges | Settings Secrets; CLI `mem secret`; secrets API | Values arrive on stdin, never argv; secrets stay on their machine | stable | Host state | Telegram bridge, Webhook receivers |
| Plugins and channels | Providers and capability plugins installed from channels; plugin views run in sandboxed frames; configure-with-AI buttons open a helper agent that sets a feature up | Marketplace and Plugins panel; CLI `plugin`; plugin and Mesh setup buttons; helper spawn API | Core providers track the app version; official core installs are trusted and never downgraded; plugin code runs confined; a helper always opens in the General workspace with the General Agent's provider, regardless of which workspace was active | stable; helper General placement in rollout | Plugin host, Agents and providers | Agent providers, Webhook receivers, Voice |
| Provider integrations | Provider hooks bind sessions to tabs | Settings; CLI `integration` | Hook content is instance-agnostic; CodeTerm rewrites only hooks it owns; install records consent | stable | Agents and providers | Session continuity |
| Configuration and themes | Settings, themes and keybindings read and written | Settings; CLI `config`, `theme` | Configuration keys use dot notation; a full dump is redacted | stable | Host state, UI | Notifications, Text hooks |

## 7. The product observes and updates itself

Detail: API-STATE, TAB-LIFECYCLE; internal essences RESILIENCE and RELEASE.

| Capability | What it enables | Entry points | Invariants | Status | Runs on | Feeds |
|---|---|---|---|---|---|---|
| Essence-anchored code map | A map generated from these essences, and later code pointing back to them, so agents can derive the project's foundation in reverse: what is core, what is peripheral, which capability a change touches, and which expectations it must re-verify | A validated map format, a Cartographer role at natural boundaries and a workspace map canvas; planned for agents rather than people | The map is an indirect beacon: work is done with an eye on it, but real architecture is more complex and is never bent to mirror the map; essences stay the stable anchor; the map is generated, never hand-drawn | planned | Plans and orchestration, UI | Canonical verification |
| Usage and Analytics | Provider quota percentages with end-of-day and end-of-cycle projections, and token usage per tab and plan aggregated in the Analytics view | Analytics titlebar view; CLI `usage`; usage API | Refresh reads the provider's quota-free source and never spends quota; each provider response counts once; a ledger rebuild is idempotent | stable; ledger rebuild in rollout | Host state, Plugin host, UI | — (sink: fed by Agent providers) |
| Health and diagnostics | Liveness, running build identity, orchestration and plan consistency | Health API; CLI `identify`, `tab inspect`, `orchestrator status`, `plan doctor` | Debugging starts by confirming the running build matches the source | stable | Service layer, Crash-recovery supervisor | Session continuity |
| Updates and local install | Signed updates checked, installed and relaunched; a local production build installed on this machine resumes the calling tab | Update UI; CLI `update check`, `update install`; build script local install | A client is offered only an artifact for its own platform; manifests are signature-verified; channel switching is disabled; local install is an explicit operation and development commands never touch production's port or CLI | stable | Service layer, CLI, Crash-recovery supervisor | Session continuity |

