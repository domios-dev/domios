# CodeTerm capability map

What CodeTerm fundamentally does, grouped by the few vectors that give the system its shape. Each row names a capability, what it makes possible, where it is reached (UI surface, CLI command group, API area), the rules that must not break, and its status. Why these vectors exist is stated once, in VISION at the repository root; how to maintain this map is stated in BEGIN_HERE.

Code is volatile and gets rebuilt in pieces; these essences are the product's stable vectors. They rarely change, and when they do they evolve gradually instead of being restructured, which is what lets them serve as the expected inputs and outputs a change must keep.

Status: **stable** is in the latest release; **in rollout** is merged for the next release and not yet shipped; **planned** is an agreed direction with no shipped surface; **roadmap** is where the product is heading, with its exact shape still open. Detail names an embedded document (read it with `codeterm docs <NAME>`) or a repository document.

## Invariants that bind every vector

- Production listens on port 7685 and is never killed by development work; development uses 7686 or a named instance.
- Core asks capabilities, never provider identities.
- A routed message is authentic only when the host placed its marker at the leading frame; the same bytes anywhere else are content.
- Coordination waits on observed events, never on sleeps or polling intervals.
- Changes to PTY or Chat input delivery need the owner's explicit approval first.
- Agent-to-agent traffic, memory and documentation are English.

## 1. Agents live and work in tabs

Detail: API-TABS, TAB-LIFECYCLE, ROUTING, HTML, AGENT-CONTROL; repository docs MODELS and the provider authoring guide.

| Capability | What it enables | Entry points | Invariants | Status |
|---|---|---|---|---|
| Workspaces and tabs | Shells and agents grouped by project; the General workspace hosts the always-on General Agent | Sidebar; CLI `workspace`, `tab`; tab and workspace API | Listing covers every workspace unless one is named; the General workspace and its agent tab are protected | stable; all-workspace default listing in rollout |
| Tab identity | Agents stay recognisable for their whole life | Spawn; CLI `tab describe` | A tab's name is set once at spawn; a task-bound tab refuses rename and records progress in its description | stable |
| Agent providers | Claude, Codex and OpenCode run in a PTY; chat-backend plugins run as open agent shells with no shell | Agent picker; CLI `agent spawn`, `agent providers`, `agent models` | Detection reads only the tab's own session; an unsupported reasoning effort refuses the launch instead of downgrading | stable; Cursor in rollout; Aider and Goose planned |
| Message delivery | Text, keys and chat turns reach a tab | CLI `send`, `tab respond`, `tab stop`; tab send and chat-send API | The payload is written at most once; once queued, a failure is only "submit unconfirmed"; the UI shows no delivery banners or retry actions | stable |
| Chat view | An agent read and driven as a conversation: tool cards, Mermaid, sandboxed HTML, file previews, voice and image attachments | Chat view; CLI `tab chat` | Rendered HTML has no network or arbitrary file access; cards follow the CLI command tree | stable; remote attachment handoff in rollout |
| Session continuity | Tabs return after restart; past sessions are searchable and resumable | CLI `sessions`, `tab rebind`, `tab repair`, `agent restart` | Restore is unconditional; a provider resumes only a session it uniquely owns; the lifecycle journal is telemetry, not authority | stable |
| Scheduling | Agent work later or on a schedule; the machine kept awake while it runs | CLI `cron`, `delayed`, `send --delay`, `keepawake`; Mesh tab crons | A cron inherits its creator's Team unless detached; keep-awake goes through CodeTerm, never OS tools directly | stable |
| Agent pad | Private per-tab scratch space for authored payloads | CLI `pad`; `--from-pad` on messaging commands | Authored bodies travel as pad files, never as inline shell text | stable |

## 2. Agents coordinate as meta-teams through plans

Detail: ORCHESTRATION, ORCHESTRATION-OPERATIONS, PLANNER, PLAN-EDITING, API-PLAN, PROMPTS.

| Capability | What it enables | Entry points | Invariants | Status |
|---|---|---|---|---|
| Teams and roles | An Orchestrator coordinates and verifies at speed; an optional Planner designs the tasks; Workers each execute one task | Plan canvas; CLI `team`, `agent spawn --role`, `orchestrator` | Two levels; Workers never spawn; only the Orchestrator approves a plan and closes a task | stable |
| Task-fitted personality | Each Worker starts from a framing that narrows its attention to its task and stays a recognisable colleague | Task executor role, role guidance, personality and model (CLI `plan task add`, `plan task update`); CLI `persona` | The Planner assigns the framing while cutting the task (the Orchestrator when planning is waived); executor role and role guidance are required and cannot be blanked; the one-word executor name is given at spawn and kept; runtime overlays add leaf kinds only and never grant spawn or Team authority | stable |
| Goals, specs, plans, tasks, strides | Durable intent: goal, what and why, how, bounded tasks, evidence | Plan canvas; CLI `plan`, `spec`, `task`, `effort`, `inbox`; plan API | Planner state is the source of truth; every task carries acceptance criteria and an executable test plan | stable |
| Dispatch and reports | One call spawns a Worker, assigns the task and delivers its brief; the Worker hands back by report | CLI `plan task dispatch`, `agent ack`, `agent report` | Every Worker turn ends in a report; only the Orchestrator marks a task done | stable |
| Review, bounce and decisions | Results accepted or bounced on evidence; owner questions routed as decision gates | CLI `plan review`, `plan decision`, `plan qa` | A bounce names the failing command and its evidence; a pass that still needs a repair is a bounce | stable |
| Reminders, milestones and pause | A plan keeps moving without polling; the owner can hold it | Heartbeat; CLI `plan milestone`, `plan pause`, `plan resume` | Plan pause is the only agent-issued pause; paused tasks are system holds and there is no per-task pause verb | stable; task-pause removal in rollout |
| Plan skills | Reusable procedures every executor of a plan reads | CLI `plan skill` | Executors read applicable skills before acting | stable |
| Team observer | A small, cheap local model quietly watches a working team in the background, reads a little of each agent's recent context on a slow cadence, and surfaces simple truths: what each agent is doing, how the team is doing, who has been stuck too long; its hints help the Orchestrator and the owner see the team without reading it | Watcher role on a watcher-capable open agent shell (charter and tick policy) | Observes and suggests, never acts on a task or holds authority; costs little enough to run continuously | roadmap |

## 3. Work flows through isolated lanes into canonical

Detail: LANES, TESTING, TEST-AUTHORING; repository docs BUILD, RELEASE and the CLI contributor guide.

| Capability | What it enables | Entry points | Invariants | Status |
|---|---|---|---|---|
| Isolated lanes | Parallel writers, each in its own worktree and lane branch | CLI `plan task dispatch`, `plan lanes` | A mutating dispatch never writes the canonical checkout or another task's lane; lane Workers are source-only: no builds, tests, dev servers or gates | stable |
| Integration | Accepted lanes land in canonical, collected per version on an integration branch | Git merge in the canonical checkout | Real merge commits only, never cherry-pick, squash or replay; a lane is deleted only after its ancestry is proven | stable |
| Canonical verification | Executable QA after merge | Build script operations; read-only verification Worker | One QA gate per physical machine at a time; tests run through the repository wrapper; Playwright runs only on the owner's explicit request | stable |
| Generated CLI artifacts | The CLI manifest, CLI reference and chat command paths track the command tree | CLI `cli-manifest --write` | Regenerated by the CLI built from the checkout being changed, never hand-edited; a contract change audits docs, prompts, skills and cards | stable |
| Local install | This machine runs a local production build and resumes the calling tab | CLI `update install --from-file`; build script local install | An explicit operation; development commands never touch production's port or CLI | stable |
| Release | Signed artifacts for macOS, Windows and Linux plus a public updater manifest | Release tag and workflows; release skill | Artifacts come from the tagged master commit; published tags never move; released master merges back into develop; public issues close only on served-byte verification | stable |

## 4. Machines join a mesh

Detail: MESH, MESH-SERVICES, MESH-BRIDGE; repository doc WEB_ACCESS.

| Capability | What it enables | Entry points | Invariants | Status |
|---|---|---|---|---|
| Pairing and trust | A flat mesh of the owner's machines | Mesh tab; CLI `mesh` | Paired is trusted and unpaired does not exist; unpair is a ban; access level is per peer and decided locally; one machine belongs to one mesh | stable |
| Remote work | Tabs, agents and plan commands on another machine; remote Workers report back across the mesh | `--mesh PEER` on CLI commands; mesh relay API | Hard rules bind on every peer; secrets never cross the mesh | stable |
| Service registry | Daemons with health probes, push tokens and an activity log | Mesh tab; CLI `service` | A push token is shown once; rotation invalidates the old token | stable |
| Pastebin | Payloads handed between machines by short id | CLI `paste` | Ephemeral, with a time to live | stable |
| Bridge migration | A bridge poller moves to another peer without restarting either daemon | CLI `bridge migrate` | Runs on the source machine only | stable |

## 5. People reach agents through bridges, voice and the web

Detail: INTEGRATIONS, API-INTEGRATIONS, TELEGRAM-SETUP, TEXT-HOOKS, TRANSCRIBE, NOTIFICATIONS.

| Capability | What it enables | Entry points | Invariants | Status |
|---|---|---|---|---|
| Telegram bridge | Conversation with the General Agent, and with individual tabs through their own topics | Settings Integrations; CLI `bridge send`, `bridge thread`, `bridge react`, `bridge restart` | A bridge-routed message is answered through the bridge, never only in the tab; one poller per bot token | stable; unified topic attachment in rollout; other services planned |
| Webhook receivers | Plugins accept inbound events and deliver them to a chosen tab | CLI `plugin receiver-target`; plugin webhook API | A receiver that authenticates reads its secret from the secret store | stable; host transcription of receiver audio in rollout |
| Text hooks | A user daemon forwards, replaces or annotates inbound bridge text before the General Agent sees it | Integrations config | Agents act on hook metadata instead of re-classifying | stable |
| Notifications | Every agent notification is kept; its level decides whether it interrupts on desktop, sound or push | Bell panel; CLI `notify` | The panel keeps every record; Workers and Planners report instead of notifying; questions to the owner use choices, never provider prompt tools | stable |
| Voice | Inbound voice becomes text through a swappable transcription service | Transcription settings; Transcriber plugin | The speech engine stays an external service behind one transcribe contract | stable; native local dictation planned |
| Web UI | The full UI from a phone or browser over the owner's Tailnet | Daemon root URL; CLI `web link` | Tailnet-only by default; the API token is the credential | stable |

## 6. The host owns memory, secrets, plugins and configuration

Detail: MEMORY, MEMORY-USAGE, MEMORY-SERVICES, SECRETS, SKILLS, CONFIG, RUNTIME-CONFIG; repository doc PLUGINS.

| Capability | What it enables | Entry points | Invariants | Status |
|---|---|---|---|---|
| Memory | Durable facts, notes, skills and preferences with hybrid search, a graph and nightly consolidation | Memory UI; CLI `mem`; memory API | English only; persistent memory lives here and never in provider side stores; bodies carry no dates | stable |
| Past-session search | What any provider's agent said, found by content | CLI `sessions search` | Conversations are searched there, durable facts in memory | stable |
| Skills | Folder skills registered once and discoverable by every agent | CLI `mem skill` | Registered through memory, never written into provider skill directories | stable |
| Secret store | Local credentials for CodeTerm, plugins and bridges | Settings Secrets; CLI `mem secret`; secrets API | Values arrive on stdin, never argv; secrets stay on their machine | stable |
| Plugins and channels | Providers and capability plugins installed from channels; plugin views run in sandboxed frames | Marketplace and Plugins panel; CLI `plugin` | Core providers track the app version; official core installs are trusted and never downgraded; plugin code runs confined | stable |
| AI setup helpers | Configure-with-AI buttons open a helper agent that sets a feature up | Plugin and Mesh setup buttons; helper spawn API | The helper opens in the General workspace with the General Agent's provider, whatever workspace is active | stable; General placement in rollout |
| Provider integrations | Provider hooks bind sessions to tabs | Settings; CLI `integration` | Hook content is instance-agnostic; CodeTerm rewrites only hooks it owns; install records consent | stable |
| Configuration and themes | Settings, themes and keybindings read and written | Settings; CLI `config`, `theme` | Dot-notation keys; a full dump is redacted | stable |

## 7. The product observes and updates itself

Detail: API-STATE, TAB-LIFECYCLE; repository docs RESILIENCE and RELEASE.

| Capability | What it enables | Entry points | Invariants | Status |
|---|---|---|---|---|
| Essence-anchored code map | Code points back to the essences it serves, so agents can derive the project's foundation in reverse: what is core, what is peripheral, which file belongs to which capability, and which expectations a change must re-verify | Planned for agents rather than people; mechanism not chosen | The map is an indirect beacon: work is done with an eye on it, but real architecture is more complex and is never bent to mirror the map; essences stay the stable anchor; the map is derived, not hand-maintained | roadmap |
| Usage tracking | Provider quota percentages with end-of-day and end-of-cycle projections | CLI `usage latest`, `usage projection`, `usage refresh` | Refresh reads the provider's quota-free source and never spends quota | stable |
| KPI ledger and Analytics | Token usage per tab and plan, aggregated in the Analytics view | Analytics titlebar view; CLI `usage rebuild-ledger`; usage API | Each provider response counts once; a rebuild is idempotent | stable; ledger rebuild in rollout |
| Health and diagnostics | Liveness, running build identity, orchestration and plan consistency | Health API; CLI `identify`, `tab inspect`, `orchestrator status`, `plan doctor` | Debugging starts by confirming the running build matches the source | stable |
| Updates | Signed updates checked, installed and relaunched | Update UI; CLI `update check`, `update install` | A client is offered only an artifact for its own platform; manifests are signature-verified; channel switching is disabled | stable |

<!-- revision: 4 -->
