import type { HelpLocaleContent } from "../types";

export const enHelpContent: HelpLocaleContent = {
  locale: "en",
  sections: [
    {
      id: "overview",
      title: "What is Domios",
      subtitle: "A terminal where AI agents live and work",
      blocks: [
        {
          type: "paragraph",
          text: "Domios is a terminal multiplexer built for AI agents. One window holds your regular shells and your agent sessions side by side: agents run real commands, coordinate with each other, remember what they learn, and reach across your machines.",
        },
        {
          type: "paragraph",
          text: "Think of it as an operating system for agents. Workspaces organize projects. Tabs run shells or agent chats. Plans hold multi-step work as durable state. Memory keeps knowledge between sessions. Mesh connects your machines into one private network. And the always-on General Agent is the machine's brain — always one tab away.",
        },
        {
          type: "list",
          items: [
            "You stay in control: agents ask before destructive actions, and every plan has explicit review gates.",
            "Everything agents do is visible — the sidebar shows who works for whom, live.",
            "Nothing is cloud-locked: providers are pluggable (Claude, Codex, OpenCode, local models).",
          ],
        },
        {
          type: "ask",
          label: "Ask the General Agent: what can you do for me on this machine?",
          question:
            "A user is reading the Domios guide and pressed 'Ask the agent' in the overview section. Introduce yourself briefly and show what you can do on this machine right now: 3-5 concrete examples based on the actual workspaces, agents, and integrations you can see. Keep it short and practical.",
        },
      ],
    },
    {
      id: "workspaces",
      title: "Workspaces, tabs",
      subtitle: "How Domios organizes your work",
      blocks: [
        {
          type: "paragraph",
          text: "A workspace groups related work — usually one project or one effort. Each workspace holds tabs, and each tab is a terminal (PTY) or an agent session. Agent tabs can flip between two faces of the same session: Chat shows the structured conversation, Terminal shows the raw PTY output underneath.",
        },
        {
          type: "list",
          items: [
            "New session inside a workspace spawns an agent or a plain shell — your last choice is remembered.",
            "Open another tab to compare output or watch two agents at once.",
            "The sidebar tree is live: working agents rise to the top, their current task shows under the name.",
            "A workspace with a plan shows the plan entry pinned first — it opens Board, List, and Canvas views.",
          ],
        },
        {
          type: "tip",
          text: "Press Cmd+K (macOS) or Ctrl+K (Windows/Linux) for the command palette — jump to tabs, spawn agents, or open settings without touching the mouse.",
        },
        {
          type: "ask",
          label: "Ask the General Agent: how should I organize my projects here?",
          question:
            "A user is reading the Domios guide (workspaces section) and pressed 'Ask the agent'. Look at their current workspaces and suggest concretely how to organize their projects in Domios: when to make a new workspace, when to open more tabs, how to name things. Short, practical, based on what actually exists on this machine.",
        },
      ],
    },
    {
      id: "agents",
      title: "Agents",
      subtitle: "The Orchestrator-led planning system, and the workers under it",
      blocks: [
        {
          type: "paragraph",
          text: "Any provider can be spawned into a tab — Claude, Codex, OpenCode, local models — for a quick one-off session. But the real power is the orchestration system: one Orchestrator owns the plan and attaches a Planner specialist when structure needs careful thought.",
        },
        {
          type: "paragraph",
          text: "The Orchestrator is the single authority: it owns the plan, dispatches Workers, keeps parallel lanes moving, reviews what comes back, and accepts or rejects changes. The Planner is a subordinate planning specialist: it drafts Spec, Plan, task briefs, and decision options when the Orchestrator delegates that work.",
        },
        { type: "figure", figureId: "agent-hierarchy" },
        {
          type: "paragraph",
          text: "Why attach a Planner? Because plans rot when execution pressure turns every design question into an improvisation. The Orchestrator hits a structural question, delegates the drafting to the Planner, then decides what lands while the lanes keep moving.",
        },
        {
          type: "list",
          items: [
            "Orchestrator — owns the plan, delegates planning work, dispatches workers in parallel worktree lanes, gates their results, and integrates branches.",
            "Planner — subordinate specialist that drafts goal, spec, plan, tasks, and decision options when asked; never runs implementation.",
            "Worker — does ONE task in its own git worktree, reports back, exits. Never spawns other agents.",
            "General Agent — the always-on machine brain: answers you anywhere, watches your messengers and plugins, and can become an Orchestrator when you ask.",
          ],
        },
        {
          type: "tip",
          text: "New session → Orchestration spawns the pair in one click: pick a provider and model for each head (defaults: Claude Opus orchestrating, Claude Fable planning) — they wake up already knowing each other.",
        },
        {
          type: "ask",
          label: "Ask the General Agent: set up an orchestration for my next project",
          question:
            "A user is reading the Domios guide (agents section) and pressed 'Ask the agent'. Explain in a few sentences how you would set up a Planner + Orchestrator pair for a new project on this machine, and offer to do it: ask what project or goal they have in mind. Do not spawn anything yet — just offer.",
        },
      ],
    },
    {
      id: "plans",
      title: "Plans",
      subtitle: "Durable orchestration state for multi-step work",
      blocks: [
        {
          type: "paragraph",
          text: "A plan is not a chat log — it is durable state that survives restarts, rate limits, and agent handoffs. It has a Goal (the north star), a Spec (what and why), a Plan (how, in waves), and Tasks, each carrying acceptance criteria, a test plan, and a correctness vector. Agents come and go; the plan remembers everything.",
        },
        { type: "figure", figureId: "plan-pipeline" },
        {
          type: "list",
          items: [
            "Board — columns by status; the fastest way to see where work stands.",
            "List — a flat stream grouped by status, with an in-flight strip when filters hide active work.",
            "Canvas — mission control: agents as live presences moving between dispatch, work floor, QA, and the gate desk.",
          ],
        },
        {
          type: "paragraph",
          text: "Tasks move pending → in progress → in review (with an optional QA stage) → done, with blocked and paused for stuck or explicitly held work. Every step is recorded as a working point — open any task to read its full activity trail: who did what, when, and why. Loops re-run a task until its stop condition holds; competitive tasks run several agents on the same problem and let the Orchestrator judge the winner.",
        },
        { type: "figure", figureId: "task-status-flow" },
        {
          type: "tip",
          text: "Workers cannot mark their own work done — they report in review, and the Orchestrator (or a QA lane) judges it. That gate is why you can trust a green board.",
        },
        {
          type: "ask",
          label: "Ask the General Agent: walk me through my current plans",
          question:
            "A user is reading the Domios guide (plans section) and pressed 'Ask the agent'. Check what plans exist on this machine (codeterm plan list) and give a short readable summary of each: goal, progress, what is currently in flight. If there are none, explain in two sentences how to start one.",
        },
      ],
    },
    {
      id: "blueprints",
      title: "Blueprints",
      subtitle: "Let agents watch for a change and act on it",
      blocks: [
        {
          type: "paragraph",
          text: "When you keep running the same commands to see whether something changed, a Blueprint can do it for you. It watches a tab, a scheduled run or a command's output and acts only when the result changes, so nobody has to poll.",
        },
        {
          type: "list",
          items: [
            "Each Blueprint reads as what is watched and what happens next.",
            "It acts within its agent's authority and asks you when an outcome is new to it.",
            "Open a Blueprint from the chat card or the workspace automation view to see its history.",
          ],
        },
        {
          type: "ask",
          label: "Ask the General Agent: turn my repeated commands into a Blueprint",
          question:
            "A user is reading the Domios guide (blueprints section) and pressed 'Ask the agent'. Ask which repeated check or command sequence they want automated, then propose one Blueprint as what is watched and what happens next, validate it with codeterm blueprint recipe and --dry-run, and create it only after they agree.",
        },
      ],
    },
    {
      id: "memory",
      title: "Memory",
      subtitle: "Facts, skills, and recall across sessions",
      blocks: [
        {
          type: "paragraph",
          text: "Agents forget everything when a session ends — unless they save it to mem, Domios's persistent memory store. It combines keyword search and semantic vectors, so both exact terms and fuzzy meaning find what was saved. Say \"remember this\" to any agent and the fact becomes recallable from every future session, by every agent.",
        },
        {
          type: "list",
          items: [
            "Facts, learnings, and decisions — saved once, found later even with different wording.",
            "Skills — reusable how-to folders with scripts; agents search them before doing unfamiliar work.",
            "Background workers enrich saved items: classification, entity extraction, knowledge-graph links, dedup.",
            "The Memory panel (sidebar) shows what is stored and what the workers are doing.",
          ],
        },
        {
          type: "tip",
          text: "Memory is machine-wide, not per-agent: a fact saved by one agent is recalled by all of them — including agents on other mesh machines when they ask.",
        },
        {
          type: "ask",
          label: "Ask the General Agent: what do you remember about me?",
          question:
            "A user is reading the Domios guide (memory section) and pressed 'Ask the agent'. Run a memory wake-up / search and summarize in a friendly, short way what you actually remember on this machine: who the user is, key projects, notable facts. If memory is nearly empty, say so and suggest what would be worth saving first.",
        },
      ],
    },
    {
      id: "supporter-agent",
      title: "Spinal Cord",
      subtitle: "Useful memories, watched tabs and bounded recovery",
      blocks: [
        {
          type: "paragraph",
          text: "The Spinal Cord helps Domios put useful memories first, notice stuck tabs and recover the General Agent within limits. Once an adapter is connected, it works automatically. There are no feature switches. Without a connected adapter, memory search and tabs work as before.",
        },
        { type: "heading", text: "What it does for you" },
        {
          type: "paragraph",
          text: "Better memory answers. When an agent searches memory, the Spinal Cord puts the most useful results first. If nothing fits well, it raises a flag, so the agent knows not to lean on a weak match.",
        },
        { type: "figure", figureId: "supporter-recall" },
        {
          type: "paragraph",
          text: "Watch tabs. It looks at recent tab text when something changes and helps Domios notice a sign-in prompt, an unavailable model or a crash. Domios checks its own evidence before taking action.",
        },
        { type: "figure", figureId: "supporter-tab-state" },
        {
          type: "paragraph",
          text: "Recover the General Agent. Domios can resume it after a confirmed crash or try a restart when startup fails. It makes at most two automatic restarts in one recovery episode, then stops and tells you. Automatic recovery starts only after its accuracy check passes; until then it asks you. Sign-in, unavailable models and uncovered folder trust need your decision; tool permissions follow the tab's own policy.",
        },
        {
          type: "paragraph",
          text: "Quiet team heartbeats. On existing scheduler ticks, Domios checks reports, task changes and each Worker's own session, ACK, provider process, turns, lane commits and classified prompt. A connected Spinal Cord makes one shared-budget decision. Healthy, unchanged work stays quiet without model calls; coarse health hashes ignore changing turn counters and timestamps. Repeated soft decisions share the existing nudge interval. Undelivered reports, failed gates, owner messages and answers, worker incidents and mode changes always wake the Orchestrator with a next step. Uncertain answers wake too. Without a model or when its budget is exhausted, changed plan evidence or unhealthy workers wake deterministically. After two hours of silence a short status confirms the scheduler is active. Review hold notes excuse deliberate review inactivity until the state or verdict changes. Relevant learnings are capped at three; fallback uses recent learnings. Settings shows wakes and suppressions in the activity feed. This is automatic, with no feature switch.",
        },
        { type: "heading", text: "Budget and fallback" },
        {
          type: "paragraph",
          text: "Model requests have no cap by default. An optional positive supporter.calls_per_hour value sets a shared rolling hourly cap; 0 also means no cap. A search can use more than one request. Jev usage is charged to your TypeSafe account; local models use your machine. Settings > Agents shows requests per hour and day and today's and this month's estimated Jev spend. Domios alerts once if Jev reports the account is out of credit. When an explicit request cap is reached, memory returns its normal results and tab observations pause until capacity is available.",
        },
        { type: "heading", text: "Connect an adapter" },
        {
          type: "list",
          ordered: true,
          items: [
            "Open Connect another adapter in Settings > Agents > Spinal Cord and choose an adapter and model. Saved connections keep their own models and tokens when you switch.",
            "Supply an API token if required, leave it empty if optional, or use no token field for a token-free adapter. Press Connect & make active.",
            "Check its status and activity there, or run codeterm supporter status. A connected adapter starts working automatically.",
          ],
        },
        { type: "settings-link", label: "Open Spinal Cord settings", target: "supporter-agent" },
        {
          type: "tip",
          text: "If an adapter is unavailable, a required token is missing or a request times out, Domios keeps normal memory search and skips the model observation. The status and activity tell you why.",
        },
        { type: "heading", text: "What it receives" },
        {
          type: "paragraph",
          text: "For memory, it receives your query and the candidate memories, with matching docs or session excerpts when configured. For tabs, it receives recent screen or chat text and runtime context. This goes to the selected adapter: TypeSafe for Jev, or your configured local server for LM Studio. Tokens stay in local protected storage; activity records keep input hashes instead of raw tab text.",
        },
        { type: "heading", text: "Which model it uses" },
        {
          type: "paragraph",
          text: "TypeSafe Jev (https://typesafe.ai) is built to judge relevance and remains the memory re-rank model. LM Studio models can help watch tabs locally without a token. They take a different approach to scoring answers and can be slower; the measured local model missed the memory deadline. Connecting a local adapter does not give it Jev-quality memory ranking.",
        },
        {
          type: "details",
          summary: "How it works: connection, requests and recovery",
          blocks: [
            {
              type: "paragraph",
              text: "A saved connection means configured, not proven reachable. Only the active adapter runs Spinal Cord requests. Removing the active connection stops the Spinal Cord without selecting another. A stale status keeps the last rows and disables changes until refresh.",
            },
            {
              type: "paragraph",
              text: "What is sent: memory queries and candidate titles and bodies, plus matching documentation or past-session excerpts if those sources are configured. Tab observations send recent screen or chat text and runtime context to the selected adapter. Jev requests go to TypeSafe; LM Studio requests go to its configured server.",
            },
            {
              type: "paragraph",
              text: "Recovery needs two agreeing observations, at least 0.90 confidence and deterministic evidence. A crash restart requires the provider process to be absent; text on a live tab alone never authorizes a restart. Automatic actions are qualified by a separate held-out evaluation; unqualified actions notify instead. The automatic provider coordinator answers classified update prompts with the declared safe option; model recovery outside it also requires agreement. Owner notifications also need two agreeing observations, each at least 0.80 confidence; observations below 0.60 are ignored. An other verdict takes no action.",
            },
            {
              type: "paragraph",
              text: "Trusted folders use Domios's existing trust responder. An uncovered folder asks you; command and tool permissions use the tab's own policy. Tabs you open yourself from the UI do not also send trust, permission or login notifications. Restored tabs with the same condition are grouped, and the same tab, condition and launch are not reported twice across restarts. Automatic recovery is journaled and sends a brief activity notification.",
            },
            {
              type: "list",
              items: [
                "Memory re-rank — it reads the top memory candidates and re-orders them so the ones that answer come first. It never removes results, and a low-confidence flag means nothing in the pool looks like a good answer. Agent and CLI searches keep the same commands.",
                "Tab state — observes recent text and runtime context on lifecycle events. A model verdict is a hint; deterministic evidence and recovery limits decide the action.",
              ],
            },
            {
              type: "paragraph",
              text: "Optional candidate sources add more to the re-rank pool. Each source has its own setting:",
            },
            {
              type: "list",
              items: [
                "Graph — related memories linked through the knowledge graph. supporter.memory_rerank.sources.graph adds up to 3 by default; 0 turns it off.",
                "Domios docs — matching sections of the built-in documentation (the same text as codeterm docs NAME), shown with a pointer to the doc. supporter.memory_rerank.sources.docs adds up to 3 by default; 0 turns it off.",
                "Sessions — matching snippets from past agent sessions. supporter.memory_rerank.sources.sessions is 0 (off) by default and allows up to 5.",
              ],
            },
            {
              type: "paragraph",
              text: "TypeSafe Jev needs an API token. Plugin decision models declare required, optional, or no token; a bare decisionModel: true declaration means optional. Optional and token-free models can drive tab-state without a token. Set a token for Jev in one of two ways:",
            },
            {
              type: "list",
              items: [
                "Use Connect another adapter, choose the adapter and model, supply a required token, and press Connect & make active. Switch saved connections with Make active; change an inactive model without activating it.",
                "From a terminal, first pipe the token from the clipboard: pbpaste | codeterm supporter token set --adapter typesafe-jev, then activate it with codeterm supporter use typesafe-jev. On other platforms pipe the token from a local secret source; never put it in argv. Run codeterm supporter status to check it.",
              ],
            },
            {
              type: "paragraph",
              text: "Candidate bodies are capped at supporter.memory_rerank.max_body_chars (2000 characters by default). Tokens are stored locally with owner-only permissions, never in config.toml, command arguments or logs. The bounded activity journal records actions, answer distributions, model identities and criteria hashes; it stores input hashes instead of raw tab text.",
            },
            {
              type: "paragraph",
              text: "supporter.calls_per_hour is optional and defaults to no cap; 0 is unlimited and a positive value caps requests across all tabs. It counts requests, not questions: a shared-state request with several questions costs one slot; per-candidate diagnostic mode costs one per candidate. Memory re-rank runs under supporter.deadline_ms (3000 ms by default). Budget accounting uses recorded request times, not a periodic reset.",
            },
            {
              type: "tip",
              text: "A failed re-rank returns baseline hits with a fallback reason; an explicit cap reached reports budget and status budget_reached with the next free time. codeterm mem search --supporter on|off is a one-search diagnostic override. Clearing a required token disconnects that adapter; optional and token-free adapters remain connected without it.",
            },
          ],
        },
        {
          type: "details",
          summary: "Decision models compared: Jev and the alternatives",
          blocks: [
            {
              type: "paragraph",
              text: "The Spinal Cord is designed for deterministic decision models such as TypeSafe Jev. Asked a pick-one (choice) or yes/no (noul) question, Jev returns a calibrated probability for each answer, and the same question always gets the same answer.",
            },
            {
              type: "list",
              items: [
                "TypeSafe Jev: built for this, with calibrated choice and noul probabilities.",
                "A Jev-style fine-tune: works approximately. Accuracy depends on how it was trained.",
                "A general LLM read through logprobs: works approximately. Less accurate, and latency depends on the model and the machine.",
              ],
            },
            {
              type: "paragraph",
              text: "Why the gap: a general model writes text. Domios reads how likely it was to write each answer, but that likelihood comes from sampling text, not from a model trained to judge. So the scores are less well calibrated, and the ranking is usually less accurate.",
            },
            {
              type: "measurement",
              label: "Measured on a local model",
              text: "The previously recorded 80-question local-model run hit the deadline on all 80 candidates (80/80). That result explains why memory re-rank stays on Jev; it is not a comparison of every LM Studio model. A change to shared-state requests must pass a fresh quality and latency comparison before shipping.",
            },
          ],
        },
        {
          type: "paragraph",
          text: "Learn more at https://docs.typesafe.ai and get a token at https://console.typesafe.ai/keys.",
        },
        {
          type: "ask",
          label: "Ask the General Agent: should I set up the Spinal Cord?",
          question:
            "A user is reading the Domios guide (Spinal Cord section) and pressed 'Ask the agent'. Check codeterm supporter status for the selected adapter, connection, token requirement, activity and shared budget. Explain how to connect it, what text it receives, Jev versus LM Studio, fallback and the two-restart recovery bound. Inspect graph, docs and session sources only if relevant. Keep it short and practical.",
        },
      ],
    },
    {
      id: "mesh",
      title: "Mesh",
      subtitle: "Your machines as one private network",
      blocks: [
        {
          type: "paragraph",
          text: "Mesh pairs your machines into a private peer network — no cloud in the middle. Every machine runs its own Domios daemon with its own General Agent; pairing lets them see each other, message each other's agents, and hand off work. Delegating a heavy task to your desktop from a laptop is one command.",
        },
        {
          type: "list",
          items: [
            "Pairing is explicit and revocable; peers authenticate with tokens, nothing is open by default.",
            "Send a message or a full task to any tab on any peer — replies route back automatically.",
            "Plans can span machines: a remote worker's reports flow home to the plan that owns the task.",
            "The Mesh tab shows peers, health probes, and an activity log of what crossed the wire.",
          ],
        },
        {
          type: "ask",
          label: "Ask the General Agent: which machines can you reach right now?",
          question:
            "A user is reading the Domios guide (mesh section) and pressed 'Ask the agent'. Check the mesh status and report which peers are currently reachable, what is running on them if visible, and one example of what the user could delegate to another machine. If mesh is off or unpaired, explain in two sentences how to enable it.",
        },
      ],
    },
    {
      id: "messengers",
      title: "Messengers",
      subtitle: "Reach your agents from Telegram",
      blocks: [
        {
          type: "paragraph",
          text: "Messengers connect a chat app to your General Agent. Telegram is built in: write to your bot from anywhere, voice messages arrive transcribed, and agents message you back when something needs you. Set it up in Settings > General > Messengers.",
        },
        {
          type: "list",
          items: [
            "Replies go back to the chat you wrote from, not to a tab.",
            "Only the Telegram user IDs you allow reach your machine.",
            "The bot token is saved to your secret store, never to a config file.",
            "To let agents act from your own Telegram or Teams account instead of a bot, see the messenger plugins section.",
          ],
        },
        { type: "settings-link", label: "Open Messengers settings", target: "telegram-bridge" },
        {
          type: "ask",
          label: "Ask the General Agent: which messengers are connected?",
          question:
            "A user is reading the Domios guide (messengers section) and pressed 'Ask the agent'. Check whether the Telegram bot bridge is connected on this machine, as which bot, and which user IDs are allowed. Show it with @ds-connect telegram. If it is not connected, say what the next step is in one sentence.",
        },
      ],
    },
    {
      id: "phone",
      title: "Answer your agents from your phone",
      subtitle: "Message your General Agent through a Telegram bot",
      blocks: [
        {
          type: "paragraph",
          text: "Connect a Telegram bot and your General Agent answers you wherever you are. Write from your phone, send voice notes that arrive transcribed, and get a message when an agent needs you.",
        },
        {
          type: "list",
          ordered: true,
          items: [
            "Create a bot with @BotFather in Telegram and copy the token it gives you.",
            "Paste the token below and press Connect. It is saved to your secret store.",
            "Add your Telegram user ID to the allowed users in Settings > General > Messengers.",
            "Send your bot a message.",
          ],
        },
        {
          type: "tip",
          text: "Only allowed user IDs reach your machine. Leave the list empty only on a bot nobody else knows about.",
        },
        {
          type: "ask",
          label: "Ask the General Agent: help me connect my phone",
          question:
            "A user is reading the Domios guide (phone section) and pressed 'Ask the agent'. Check whether the Telegram bot bridge is connected, whether a token is stored and which user IDs are allowed. Walk them through the next missing step only, one step at a time, and show @ds-connect telegram while it is not connected.",
        },
        { type: "connect", bridgeId: "telegram" },
      ],
    },
    {
      id: "messenger-plugins",
      title: "Let agents use your Telegram and Teams",
      subtitle: "Plugins that act from your own messenger accounts",
      blocks: [
        {
          type: "paragraph",
          text: "The bot bridge talks as a bot. These plugins sign in as you, so agents can find a chat by name, read recent history, search messages and send text or files from your own account.",
        },
        { type: "plugin", pluginId: "telegram-client" },
        { type: "plugin", pluginId: "teams-client" },
        {
          type: "list",
          items: [
            "Install runs only when you press it, the same as in Plugins.",
            "After installing, open the plugin to sign in. Your session stays on this machine.",
            "Agents send from your account only when you ask them to.",
          ],
        },
        {
          type: "ask",
          label: "Ask the General Agent: what can you do with my messengers?",
          question:
            "A user is reading the Domios guide (messenger plugins section) and pressed 'Ask the agent'. Check whether telegram-client and teams-client are installed and signed in on this machine. Show each with @ds-plugin and give two concrete things you could do for them with the ones that are ready.",
        },
      ],
    },
    {
      id: "tips",
      title: "Tips & shortcuts",
      subtitle: "Everyday commands and where to look when things break",
      blocks: [
        {
          type: "paragraph",
          text: "Most navigation is faster from the command palette (Cmd/Ctrl+K). Configurable keybindings below reflect your current settings; the rest are fixed app shortcuts.",
        },
        {
          type: "shortcut-table",
          rows: [
            { label: "Command palette", source: { kind: "app", id: "command_palette" } },
            { label: "New tab", source: { kind: "app", id: "new_tab" } },
            { label: "Close tab", source: { kind: "app", id: "close_tab" } },
            { label: "New workspace", source: { kind: "app", id: "new_workspace" } },
            { label: "Settings", source: { kind: "app", id: "settings" } },
            { label: "Toggle sidebar", source: { kind: "app", id: "toggle_sidebar" } },
            { label: "Next tab / plan entry", source: { kind: "config", key: "next_tab" } },
            { label: "Previous tab / plan entry", source: { kind: "config", key: "prev_tab" } },
            { label: "Reopen closed tab", source: { kind: "config", key: "reopen_tab" } },
          ],
        },
        {
          type: "list",
          items: [
            "Logs — Settings → Logs streams daemon, mesh, agent, and integration diagnostics, filterable live.",
            "Updates — Settings → Check for updates; the About row shows exactly what build you run.",
            "An agent looks stuck? Open its Terminal face — the raw PTY usually shows why.",
            "Esc closes modals, including this guide.",
          ],
        },
        {
          type: "ask",
          label: "Ask the General Agent: give me your top tips for this machine",
          question:
            "A user is reading the Domios guide (tips section) and pressed 'Ask the agent'. Based on how this machine is actually used (workspaces, plans, integrations you can see), give your top 5 practical Domios tips for THIS user specifically. Short bullets, no generic advice.",
        },
      ],
    },
  ],
};
