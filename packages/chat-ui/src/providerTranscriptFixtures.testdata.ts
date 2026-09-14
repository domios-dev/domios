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
    diffNeedle?: string;
    diffRemovedNeedle?: string;
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
    content: "Skill context: use the codeterm workflow for Tab reports.",
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
      "| `codeterm tab list` | documented |",
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
    toolKind: "command",
    toolSummary: "rg \"codeterm send\" docs",
    toolDetail: "rg \"codeterm send\" docs",
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
    content: "",
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
    toolKind: "edit",
    toolSummary: "docs/API.md",
    toolDetail: "docs/API.md",
    toolEdits: [{ path: "docs/API.md", old: "", new: "## CLI\n\nReport with codeterm send.\n" }],
  },
];

/** Real-session apply_patch hunk on cli/src/cmd/mem/mod.rs (trimmed from rollout JSONL). */
export const codexMemModPatch = [
  "*** Begin Patch",
  "*** Update File: cli/src/cmd/mem/mod.rs",
  "@@",
  "     let text = resp.text().unwrap_or_default();",
  "     let val: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();",
  "     if !status.is_success() {",
  "-        return Err(if text.is_empty() {",
  "-            val.to_string()",
  "+        let body = text.trim();",
  "+        return Err(if body.is_empty() {",
  "+            status.to_string()",
  "         } else {",
  "-            text",
  "+            format!(\"{} {}\", status, body)",
  "         });",
  "     }",
  "     Ok(val)",
  " }",
  "*** End Patch",
].join("\n");

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
    toolKind: "command",
    toolSummary: "pnpm exec tsx packages/chat-ui/src/MessageBubble.providers.test.tsx",
    toolDetail: "pnpm exec tsx packages/chat-ui/src/MessageBubble.providers.test.tsx",
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
    content: "",
    id: "codex-tool-call-2",
    provider: "codex",
    type: "tool_call",
    toolName: "apply_patch",
    toolId: "call_codex_2",
    toolInput: {
      patch: codexMemModPatch,
    },
    toolArgs: '{"patch":"*** Begin Patch"}',
    toolKind: "edit",
    toolSummary: "cli/src/cmd/mem/mod.rs",
    toolDetail: "cli/src/cmd/mem/mod.rs",
    toolEdits: [{
      path: "cli/src/cmd/mem/mod.rs",
      old: "return Err(if text.is_empty() {\nval.to_string()\ntext",
      new: "let body = text.trim();\nreturn Err(if body.is_empty() {\nstatus.to_string()\nformat!(\"{} {}\", status, body)",
    }],
  },
  {
    id: "codex-tool-result-2",
    provider: "codex",
    type: "tool_result",
    toolId: "call_codex_2",
    content: "Success. Updated the following files:\nM cli/src/cmd/mem/mod.rs\n",
    toolResult: "Success. Updated the following files:\nM cli/src/cmd/mem/mod.rs\n",
  },
];

export const codexStructuredEditMessages: NormalizedChatMessage[] = [
  {
    id: "codex-rg-search",
    provider: "codex",
    type: "tool_call",
    toolName: "exec_command",
    toolId: "call_rg_1",
    toolInput: { cmd: 'rg -n "mem_post_request" cli/src' },
    toolArgs: '{"cmd":"rg -n \\"mem_post_request\\" cli/src"}',
    content: 'rg -n "mem_post_request" cli/src',
    toolKind: "command",
    toolSummary: 'rg -n "mem_post_request" cli/src',
    toolDetail: 'rg -n "mem_post_request" cli/src',
  },
  {
    content: "",
    id: "codex-structured-apply-patch",
    provider: "codex",
    type: "tool_call",
    toolName: "apply_patch",
    toolId: "call_zegiw0ZHuMspcZ1NRWT7WcWQ",
    toolInput: { patch: codexMemModPatch },
    toolArgs: '{"patch":"*** Begin Patch"}',
    toolKind: "edit",
    toolSummary: "cli/src/cmd/mem/mod.rs",
    toolDetail: "cli/src/cmd/mem/mod.rs",
    toolEdits: [{
      path: "cli/src/cmd/mem/mod.rs",
      old: "return Err(if text.is_empty() {\nval.to_string()\ntext",
      new: "let body = text.trim();\nreturn Err(if body.is_empty() {\nstatus.to_string()\nformat!(\"{} {}\", status, body)",
    }],
  },
  {
    id: "codex-structured-patch-result",
    provider: "codex",
    type: "tool_result",
    toolId: "call_zegiw0ZHuMspcZ1NRWT7WcWQ",
    content: "Success. Updated the following files:\nM cli/src/cmd/mem/mod.rs\n",
    toolResult: "Success. Updated the following files:\nM cli/src/cmd/mem/mod.rs\n",
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
    toolKind: "command",
    toolSummary: "cat packages/chat-ui/src/MessageBubble.tsx",
    toolDetail: "cat packages/chat-ui/src/MessageBubble.tsx",
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
    content: "",
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
    toolKind: "read",
    toolSummary: "packages/chat-ui/src/MessageBubble.tsx",
    toolDetail: "packages/chat-ui/src/MessageBubble.tsx",
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
      toolLabels: ["Search", "edit"],
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
      toolLabels: ["command", "edit"],
      resultNeedle: "Exit code: 0",
      diffNeedle: "let body = text.trim()",
      diffRemovedNeedle: "val.to_string()",
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
      toolLabels: ["Read"],
      resultNeedle: "export function MessageBubble",
    },
  },
];

/** Normalized provider casing reaches the shared wait predicate without parser-specific UI logic. */
