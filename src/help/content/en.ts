import type { HelpLocaleContent } from "../types";

export const enHelpContent: HelpLocaleContent = {
  locale: "en",
  sections: [
    {
      id: "overview",
      title: "What is CodeTerm",
      subtitle: "A terminal where AI agents live and work",
      blocks: [
        {
          type: "paragraph",
          text: "CodeTerm is a terminal multiplexer built for AI agents. One window holds your regular shells and your agent sessions side by side: agents run real commands, coordinate with each other, remember what they learn, and reach across your machines.",
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
            "A user is reading the CodeTerm guide and pressed 'Ask the agent' in the overview section. Introduce yourself briefly and show what you can do on this machine right now: 3-5 concrete examples based on the actual workspaces, agents, and integrations you can see. Keep it short and practical.",
        },
      ],
    },
    {
      id: "workspaces",
      title: "Workspaces, tabs",
      subtitle: "How CodeTerm organizes your work",
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
            "A user is reading the CodeTerm guide (workspaces section) and pressed 'Ask the agent'. Look at their current workspaces and suggest concretely how to organize their projects in CodeTerm: when to make a new workspace, when to open more tabs, how to name things. Short, practical, based on what actually exists on this machine.",
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
            "General Agent — the always-on machine brain: answers you anywhere, watches integrations, and can become an Orchestrator when you ask.",
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
            "A user is reading the CodeTerm guide (agents section) and pressed 'Ask the agent'. Explain in a few sentences how you would set up a Planner + Orchestrator pair for a new project on this machine, and offer to do it: ask what project or goal they have in mind. Do not spawn anything yet — just offer.",
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
            "A user is reading the CodeTerm guide (plans section) and pressed 'Ask the agent'. Check what plans exist on this machine (codeterm plan list) and give a short readable summary of each: goal, progress, what is currently in flight. If there are none, explain in two sentences how to start one.",
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
          text: "Agents forget everything when a session ends — unless they save it to mem, CodeTerm's persistent memory store. It combines keyword search and semantic vectors, so both exact terms and fuzzy meaning find what was saved. Say \"remember this\" to any agent and the fact becomes recallable from every future session, by every agent.",
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
            "A user is reading the CodeTerm guide (memory section) and pressed 'Ask the agent'. Run a memory wake-up / search and summarize in a friendly, short way what you actually remember on this machine: who the user is, key projects, notable facts. If memory is nearly empty, say so and suggest what would be worth saving first.",
        },
      ],
    },
    {
      id: "supporter-agent",
      title: "Supporter Agent",
      subtitle: "A small helper that picks the best answers for your agents",
      blocks: [
        {
          type: "paragraph",
          text: "The Supporter Agent is a small, fast helper model. It makes quick choices for CodeTerm, like which saved memory best answers a question. It never runs tasks, types into your tabs or writes code.",
        },
        { type: "heading", text: "What it does for you" },
        {
          type: "paragraph",
          text: "Better memory answers. When an agent searches memory, the Supporter puts the most useful results first. If nothing fits well, it raises a flag, so the agent knows not to lean on a weak match.",
        },
        { type: "figure", figureId: "supporter-recall" },
        {
          type: "paragraph",
          text: "A second look at starting tabs, off by default. It reads a new tab's screen and notes whether it looks fine or seems stuck, for example on a sign-in prompt.",
        },
        { type: "figure", figureId: "supporter-tab-state" },
        { type: "heading", text: "What it costs" },
        {
          type: "paragraph",
          text: "About $0.0007 per memory search, so roughly 70 cents for a thousand searches. It is charged to your TypeSafe account.",
        },
        { type: "heading", text: "Turn it on or off" },
        {
          type: "list",
          ordered: true,
          items: [
            "Get a token at https://console.typesafe.ai/keys.",
            "Open the Supporter Agent settings, paste the token and press Save token.",
            "Turn on Re-rank memory recall in Memory > Settings. Switch it off there any time.",
          ],
        },
        { type: "settings-link", label: "Open Supporter Agent settings", target: "supporter-agent" },
        {
          type: "tip",
          text: "If the Supporter is slow, down or has no token, CodeTerm quietly uses its normal search. Nothing breaks.",
        },
        { type: "heading", text: "Which model it uses" },
        {
          type: "paragraph",
          text: "It works best with a model built for decisions, such as TypeSafe Jev (https://typesafe.ai), the default. Other models can stand in, with rougher answers.",
        },
        {
          type: "details",
          summary: "How it works: what is sent, timing and fallback",
          blocks: [
            {
              type: "list",
              items: [
                "Memory re-rank — it reads the top memory candidates and re-orders them so the ones that answer come first. It never removes results, and a low-confidence flag means nothing in the pool looks like a good answer. Agent and CLI searches keep the same commands.",
                "Tab state — shadow logging only. When enabled, it reads a starting tab's recent screen and writes one log line beside CodeTerm's own verdict. It changes nothing yet and is off by default.",
              ],
            },
            {
              type: "paragraph",
              text: "Optional candidate sources add more to the re-rank pool. Each source has its own setting:",
            },
            {
              type: "list",
              items: [
                "Graph — related memories linked through the knowledge graph. memory.decision_rerank.sources.graph adds up to 3 by default; 0 turns it off.",
                "CodeTerm docs — matching sections of the built-in documentation (the same text as codeterm docs NAME), shown with a pointer to the doc. memory.decision_rerank.sources.docs adds up to 3 by default; 0 turns it off.",
                "Sessions — matching snippets from past agent sessions. memory.decision_rerank.sources.sessions is 0 (off) by default and allows up to 5.",
              ],
            },
            {
              type: "paragraph",
              text: "It only needs an API token. Set it up in one of two ways:",
            },
            {
              type: "list",
              items: [
                "In Settings > Agents > Supporter Agent, paste the token and press Save token. Then turn on Re-rank memory recall in Memory > Settings.",
                "From a terminal, pipe it in: pbpaste | codeterm mem decision token set, then codeterm config set memory.decision_rerank.enabled true. Run codeterm mem decision status to check it.",
              ],
            },
            {
              type: "paragraph",
              text: "What is sent: for a search, your query plus each candidate's title and body, with bodies capped at memory.decision_rerank.max_body_chars (2000 characters by default). For tab state, the tab's recent screen text. The token is stored locally with owner-only 0600 permissions and is never shown again or written to logs.",
            },
            {
              type: "paragraph",
              text: "Cost is about $0.0007 per search. The whole step runs under one deadline, memory.decision_rerank.deadline_ms (3000 ms by default).",
            },
            {
              type: "tip",
              text: "On a timeout, an error, or with no token, CodeTerm returns the normal search results. To turn it off, switch off Re-rank memory recall in Memory > Settings or run codeterm config set memory.decision_rerank.enabled false; one search can skip it with --decision-rerank off. Clear token removes the token entirely.",
            },
          ],
        },
        {
          type: "details",
          summary: "Decision models compared: Jev and the alternatives",
          blocks: [
            {
              type: "paragraph",
              text: "The Supporter Agent is designed for deterministic decision models such as TypeSafe Jev. Asked a pick-one (choice) or yes/no (noul) question, Jev returns a calibrated probability for each answer, and the same question always gets the same answer.",
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
              text: "Why the gap: a general model writes text. CodeTerm reads how likely it was to write each answer, but that likelihood comes from sampling text, not from a model trained to judge. So the scores are less well calibrated, and the ranking is usually less accurate.",
            },
            {
              type: "measurement",
              label: "Measured on a local model",
              text: "Numbers from the LM Studio comparison will appear here.",
            },
          ],
        },
        {
          type: "paragraph",
          text: "Learn more at https://docs.typesafe.ai and get a token at https://console.typesafe.ai/keys.",
        },
        {
          type: "ask",
          label: "Ask the General Agent: should I set up the Supporter Agent?",
          question:
            "A user is reading the CodeTerm guide (Supporter Agent section) and pressed 'Ask the agent'. Check with codeterm mem decision status whether a token is set, and whether memory.decision_rerank.enabled and its graph, docs and sessions sources are on. Explain what text is sent when it is on and what it costs. Keep it short and practical.",
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
          text: "Mesh pairs your machines into a private peer network — no cloud in the middle. Every machine runs its own CodeTerm daemon with its own General Agent; pairing lets them see each other, message each other's agents, and hand off work. Delegating a heavy task to your desktop from a laptop is one command.",
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
            "A user is reading the CodeTerm guide (mesh section) and pressed 'Ask the agent'. Check the mesh status and report which peers are currently reachable, what is running on them if visible, and one example of what the user could delegate to another machine. If mesh is off or unpaired, explain in two sentences how to enable it.",
        },
      ],
    },
    {
      id: "integrations",
      title: "Integrations",
      subtitle: "Bridges to the outside world",
      blocks: [
        {
          type: "paragraph",
          text: "Bridges connect outside channels to your agents. The Telegram bridge is the flagship: message your General Agent from your phone, get answers, voice messages are transcribed before delivery, and agents can push notifications back when something needs you.",
        },
        {
          type: "list",
          items: [
            "Inbound messages carry a return address — the agent answers in the channel you wrote from, not in the tab.",
            "Allowed-user lists gate who can talk to your machine; tokens live in the secret store, not in configs.",
            "Discord and other bridges follow the same pattern when enabled; text hooks let you intercept and route messages programmatically.",
          ],
        },
        {
          type: "ask",
          label: "Ask the General Agent: what integrations are live here?",
          question:
            "A user is reading the CodeTerm guide (integrations section) and pressed 'Ask the agent'. Check which integrations/bridges are configured and enabled on this machine and summarize their status. If none are set up, recommend which one to start with and where the setup lives in Settings.",
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
            "A user is reading the CodeTerm guide (tips section) and pressed 'Ask the agent'. Based on how this machine is actually used (workspaces, plans, integrations you can see), give your top 5 practical CodeTerm tips for THIS user specifically. Short bullets, no generic advice.",
        },
      ],
    },
  ],
};
