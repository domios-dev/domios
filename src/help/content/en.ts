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
          text: "CodeTerm is a terminal where AI agents live and work. It multiplexes regular shells and AI agent sessions in one window, lets agents coordinate with each other, remember what they learn, and reach across your machines.",
        },
        {
          type: "paragraph",
          text: "Think of it as an operating system for agents: workspaces organize your projects, panes run shells or chats, and the always-on General Agent keeps the machine aware of what is happening.",
        },
      ],
    },
    {
      id: "workspaces",
      title: "Workspaces, tabs & panes",
      subtitle: "How CodeTerm organizes your work",
      blocks: [
        {
          type: "paragraph",
          text: "Workspaces group related work — usually one project or effort per workspace. Each workspace has tabs, and each tab holds a pane: a real terminal (PTY) or an agent chat view.",
        },
        {
          type: "list",
          items: [
            "Split panes side-by-side to compare output or run parallel agents.",
            "Toggle Chat / Terminal on agent tabs when you want raw PTY output or the structured chat UI.",
            "Use the sidebar tree to see which agents are active and how they relate.",
          ],
        },
        {
          type: "tip",
          text: "Press Cmd+K (macOS) or Ctrl+K (Windows/Linux) to open the command palette — jump to tabs, spawn agents, or open settings without leaving the keyboard.",
        },
      ],
    },
    {
      id: "agents",
      title: "Agents",
      subtitle: "Spawn, coordinate, and supervise AI workers",
      blocks: [
        {
          type: "paragraph",
          text: "Spawn agents (Claude, Codex, OpenCode, and other providers) into panes. The always-on General Agent is the machine's brain — it watches workspaces, schedules work, and can delegate when you ask.",
        },
        {
          type: "paragraph",
          text: "For multi-step work, CodeTerm uses a two-level hierarchy: an Orchestrator decomposes goals and spawns Workers; each Worker does one concrete task, reports back, and exits. The sidebar tree shows who is working for whom, live.",
        },
        { type: "figure", figureId: "agent-hierarchy" },
        {
          type: "list",
          items: [
            "Orchestrator — top of the chain; delegates by spawning workers with explicit roles.",
            "Worker — leaf agent; never spawns other agents; runs codeterm agent report when done.",
            "General Agent — persistent daemon pane; not a substitute for task-scoped workers.",
          ],
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
          text: "Plans capture durable orchestration state: a Goal (north star), a Spec (what and why), a Plan (how, in waves), and Tasks with acceptance criteria and test plans.",
        },
        { type: "figure", figureId: "plan-pipeline" },
        {
          type: "list",
          items: [
            "Board — columns by task status; good for kanban-style flow.",
            "List — chronological stream of tasks and milestones.",
            "Canvas — spatial view linking spec, plan, and tasks on a board.",
          ],
        },
        {
          type: "paragraph",
          text: "Tasks move through a lifecycle: pending → in progress → in review (and QA) → done. Use blocked or parked when work is stuck or deferred. Working points record what happened; loops re-run a task until its stop condition is met.",
        },
        { type: "figure", figureId: "task-status-flow" },
      ],
    },
    {
      id: "memory",
      title: "Memory",
      subtitle: "Facts, skills, and recall across sessions",
      blocks: [
        {
          type: "paragraph",
          text: "Agents save durable facts, learnings, and skills to mem — CodeTerm's hybrid keyword + semantic memory store. The Memory panel shows indexed items and background workers (Librarian, Dreamer).",
        },
        {
          type: "list",
          items: [
            "Ask any agent to remember something — it becomes searchable later.",
            "Skills live in indexed folders; register project skills with codeterm mem skill add.",
            "Wake-up recall injects relevant context when a workspace session starts.",
          ],
        },
        {
          type: "tip",
          text: "Use codeterm mem search from the CLI or ask an agent to recall before starting unfamiliar work.",
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
          text: "Pair machines into a private mesh: see peers, send messages and tasks to their agents, and share plans across hosts. Enable mesh per machine from the Mesh tab in the sidebar.",
        },
        {
          type: "list",
          items: [
            "Each peer runs its own daemon; pairing is explicit and revocable.",
            "Remote panes and agent spawns target a chosen peer.",
            "Health probes and activity logs help you see what is reachable.",
          ],
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
          text: "Bridges such as Telegram route external messages to your agents and send replies back. Configure tokens, routing rules, and notification behavior in Settings → Integrations.",
        },
        {
          type: "list",
          items: [
            "Inbound messages can spawn or resume agent sessions.",
            "Outbound replies come from the agent that handled the thread.",
            "Discord and other bridges follow the same pattern when enabled.",
          ],
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
          text: "Most navigation is faster from the command palette (Cmd/Ctrl+K). Configurable keybindings below reflect your current settings; others are fixed app shortcuts.",
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
            "Logs — Settings → Logs for daemon, mesh, agent, and integration diagnostics.",
            "Updates — Settings → Check for updates; dev builds show a git hash in /api/health.",
            "Esc closes modals including Settings.",
          ],
        },
      ],
    },
  ],
};
