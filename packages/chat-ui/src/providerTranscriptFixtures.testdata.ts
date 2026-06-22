import type { NormalizedChatMessage } from "@codeterm/chat-core";

export type ProviderTranscriptId = "claude" | "codex" | "opencode";

export interface ProviderTranscriptFixture {
  provider: ProviderTranscriptId;
  messages: NormalizedChatMessage[];
  expected: {
    attachmentName: string;
    codeNeedle: string;
    linkHref: string;
    mermaidNeedle: string;
    systemLabel: string;
    thinkingNeedle: string;
    toolLabels: string[];
    resultNeedle: string;
  };
}

const claudeMessages: NormalizedChatMessage[] = [
  {
    id: "claude-user-1",
    provider: "claude",
    type: "user",
    content: "Please inspect claude-terminal.png and update the CLI docs.",
    attachments: [
      {
        id: "claude-attachment-1",
        mimeType: "image/png",
        data: "iVBORw0KGgo=",
        path: "artifacts/claude-terminal.png",
      },
    ],
  },
  {
    id: "claude-system-1",
    provider: "claude",
    type: "system",
    systemSubtype: "skill_context",
    content: "Skill context: use the codeterm workflow for pane reports.",
  },
  {
    id: "claude-assistant-1",
    provider: "claude",
    type: "assistant",
    content: [
      "The docs need one command table.",
      "",
      "| Command | Status |",
      "| --- | --- |",
      "| `codeterm pane list` | documented |",
      "| `codeterm send` | documented |",
      "",
      "See [CodeTerm docs](https://example.test/claude-docs).",
      "",
      "```ts",
      "export const claudeReady = true;",
      "```",
      "",
      "```mermaid",
      "flowchart TD",
      "  claudeStart[Claude parser] --> claudeRender[MessageBubble]",
      "```",
    ].join("\n"),
  },
  {
    id: "claude-thinking-1",
    provider: "claude",
    type: "thinking",
    collapsed: true,
    content: "Claude reasoning block selected Bash before Write.",
  },
  {
    id: "claude-tool-call-1",
    provider: "claude",
    type: "tool_call",
    toolName: "Bash",
    toolId: "toolu_claude_1",
    toolInput: {
      command: "rg \"codeterm send\" docs",
      description: "Find reporting docs",
    },
    toolArgs: "rg \"codeterm send\" docs",
    content: "rg \"codeterm send\" docs",
  },
  {
    id: "claude-tool-result-1",
    provider: "claude",
    type: "tool_result",
    toolId: "toolu_claude_1",
    content: "docs/review-2026-06/tasks/README.md: report per protocol",
    toolResult: "docs/review-2026-06/tasks/README.md: report per protocol",
  },
  {
    id: "claude-tool-call-2",
    provider: "claude",
    type: "tool_call",
    toolName: "Write",
    toolId: "toolu_claude_2",
    toolInput: {
      file_path: "docs/API.md",
      content: "## CLI\n\nReport with codeterm send.\n",
    },
    toolArgs: '{"file_path":"docs/API.md"}',
  },
];

const codexMessages: NormalizedChatMessage[] = [
  {
    id: "codex-system-1",
    provider: "codex",
    type: "system",
    systemSubtype: "meta",
    content: "<environment_context><cwd>C:/Developer/codeterm</cwd></environment_context>",
  },
  {
    id: "codex-user-1",
    provider: "codex",
    type: "user",
    content: "Render this rollout with the attached trace.txt file.",
    attachments: [
      {
        id: "codex-attachment-1",
        mimeType: "text/plain",
        path: "rollout/trace.txt",
      },
    ],
  },
  {
    id: "codex-assistant-1",
    provider: "codex",
    type: "assistant",
    content: [
      "Rollout rendering summary:",
      "",
      "| Item | Result |",
      "| --- | --- |",
      "| shell command | captured |",
      "| patch | summarized |",
      "",
      "Read the [fixture note](https://example.test/codex-fixture).",
      "",
      "```python",
      "codex_ready = True",
      "```",
      "",
      "```mermaid",
      "sequenceDiagram",
      "  participant Codex",
      "  participant UI",
      "  Codex->>UI: normalized messages",
      "```",
    ].join("\n"),
  },
  {
    id: "codex-thinking-1",
    provider: "codex",
    type: "thinking",
    collapsed: true,
    content: "Codex reasoning summary kept shell_command before apply_patch.",
  },
  {
    id: "codex-tool-call-1",
    provider: "codex",
    type: "tool_call",
    toolName: "shell_command",
    toolId: "call_codex_1",
    toolInput: {
      command: "pnpm exec tsx packages/chat-ui/src/MessageBubble.providers.test.tsx",
      timeout_ms: 10000,
    },
    toolArgs: "pnpm exec tsx packages/chat-ui/src/MessageBubble.providers.test.tsx",
    content: "pnpm exec tsx packages/chat-ui/src/MessageBubble.providers.test.tsx",
  },
  {
    id: "codex-tool-result-1",
    provider: "codex",
    type: "tool_result",
    toolId: "call_codex_1",
    content: "Exit code: 0\nWall time: 1.2 seconds",
    toolResult: "Exit code: 0\nWall time: 1.2 seconds",
  },
  {
    id: "codex-tool-call-2",
    provider: "codex",
    type: "tool_call",
    toolName: "apply_patch",
    toolId: "call_codex_2",
    toolInput: {
      old_string: "const covered = false;",
      new_string: "const covered = true;",
    },
    toolArgs: '{"old_string":"const covered = false;","new_string":"const covered = true;"}',
  },
];

const opencodeMessages: NormalizedChatMessage[] = [
  {
    id: "opencode-system-1",
    provider: "opencode",
    type: "system",
    systemSubtype: "meta",
    content: "OpenCode session resumed from project metadata.",
  },
  {
    id: "opencode-user-1",
    provider: "opencode",
    type: "user",
    content: "Check the renderer with the opencode-run.log attachment.",
    attachments: [
      {
        id: "opencode-attachment-1",
        mimeType: "text/plain",
        path: "logs/opencode-run.log",
      },
    ],
  },
  {
    id: "opencode-thinking-1",
    provider: "opencode",
    type: "thinking",
    collapsed: true,
    content: "OpenCode reasoning part chose bash and read tools.",
  },
  {
    id: "opencode-tool-call-1",
    provider: "opencode",
    type: "tool_call",
    toolName: "bash",
    toolId: "opencode_tool_1",
    toolInput: {
      command: "cat packages/chat-ui/src/MessageBubble.tsx",
    },
    toolArgs: "cat packages/chat-ui/src/MessageBubble.tsx",
    content: "cat packages/chat-ui/src/MessageBubble.tsx",
    toolResult: "export function MessageBubble() { /* trimmed */ }",
  },
  {
    id: "opencode-tool-result-1",
    provider: "opencode",
    type: "tool_result",
    toolId: "opencode_tool_1",
    content: "export function MessageBubble() { /* trimmed */ }",
    toolResult: "export function MessageBubble() { /* trimmed */ }",
  },
  {
    id: "opencode-assistant-1",
    provider: "opencode",
    type: "assistant",
    content: [
      "OpenCode renderer summary:",
      "",
      "| Part | UI branch |",
      "| --- | --- |",
      "| reasoning | thinking |",
      "| tool | tool call |",
      "",
      "Review the [OpenCode fixture](https://example.test/opencode-fixture).",
      "",
      "```tsx",
      "export const OpenCodeCard = () => <span>ready</span>;",
      "```",
      "",
      "```mermaid",
      "flowchart LR",
      "  openCodeTool[OpenCode tool] --> openCodeBubble[MessageBubble]",
      "```",
    ].join("\n"),
  },
  {
    id: "opencode-tool-call-2",
    provider: "opencode",
    type: "tool_call",
    toolName: "read",
    toolId: "opencode_tool_2",
    toolInput: {
      file_path: "packages/chat-ui/src/MessageBubble.tsx",
      content: "export function MessageBubble() {\n  return null;\n}\n",
    },
    toolArgs: '{"file_path":"packages/chat-ui/src/MessageBubble.tsx"}',
  },
];

export const providerTranscriptFixtures: ProviderTranscriptFixture[] = [
  {
    provider: "claude",
    messages: claudeMessages,
    expected: {
      attachmentName: "claude-terminal.png",
      codeNeedle: "claudeReady",
      linkHref: "https://example.test/claude-docs",
      mermaidNeedle: "claudeStart",
      systemLabel: "skill context",
      thinkingNeedle: "Claude reasoning block",
      toolLabels: ["Bash", "Write"],
      resultNeedle: "report per protocol",
    },
  },
  {
    provider: "codex",
    messages: codexMessages,
    expected: {
      attachmentName: "trace.txt",
      codeNeedle: "codex_ready",
      linkHref: "https://example.test/codex-fixture",
      mermaidNeedle: "sequenceDiagram",
      systemLabel: "meta",
      thinkingNeedle: "Codex reasoning summary",
      toolLabels: ["shell", "apply_patch"],
      resultNeedle: "Exit code: 0",
    },
  },
  {
    provider: "opencode",
    messages: opencodeMessages,
    expected: {
      attachmentName: "opencode-run.log",
      codeNeedle: "OpenCodeCard",
      linkHref: "https://example.test/opencode-fixture",
      mermaidNeedle: "openCodeTool",
      systemLabel: "meta",
      thinkingNeedle: "OpenCode reasoning part",
      toolLabels: ["bash", "read"],
      resultNeedle: "export function MessageBubble",
    },
  },
];
