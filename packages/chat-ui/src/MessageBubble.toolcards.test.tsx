// Run: npx tsx packages/chat-ui/src/MessageBubble.toolcards.test.tsx
// Tool cards: exec, file, image, structured inbound and the collapse rules that
// govern them. Provider transcript normalization lives in the sibling suite.

import { act } from "react";

import { MessageBubble } from "./MessageBubble";
import { parseInboundReport } from "./codeterm/InboundReportCard";
import { CROSS_TAB_BUBBLE_TESTID } from "./crossTabBubble";
import { reduceToolEvents } from "@codeterm/chat-core";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import {
  assert,
  assertIncludes,
  assertLink,
  assertToolCard,
  buttons,
  clickButtonContaining,
  clipboardWrites,
  createSuite,
  expandToolCard,
  mermaidSources,
  renderMessages,
  renderTranscript,
  textOf,
  toolCall,
  toolCardFor,
  waitForEffects,
  windowAny,
} from "./MessageBubble.providers.testkit";

const { test, run } = createSuite("MessageBubble tool card");

test("chatBackend exec tool_call renders command card and attached result", async () => {
  const { host, root } = await renderMessages([
    {
      id: "cb-tool-call-1",
      provider: "lmstudio",
      type: "tool_call",
      content: "",
      toolName: "exec",
      toolId: "call-1",
      toolInput: { cmd: "npm test -- --runInBand", cwd: "/repo/app" },
      toolArgs: JSON.stringify({ cmd: "npm test -- --runInBand", cwd: "/repo/app" }),
      toolKind: "command",
      toolSummary: "npm test -- --runInBand",
      toolDetail: "npm test -- --runInBand\ncwd /repo/app",
    },
    {
      id: "cb-tool-result-1",
      provider: "lmstudio",
      type: "tool_result",
      content: "Exit code: 0\nTests passed",
      toolResult: "Exit code: 0\nTests passed",
      toolId: "call-1",
    },
  ]);
  try {
    assert(host.querySelector('[data-testid="chat-tool-call"]'), "expected command card");
    const commandCard = host.querySelector('[data-testid="chat-tool-call"]') as HTMLElement;
    assert(commandCard.querySelector("svg"), "command card should render its glyph");
    assert(commandCard.textContent?.includes("command"), "command card should render its label");
    assertIncludes(host, "npm test -- --runInBand", "exec card command");
    assert(!commandCard.querySelector('[data-testid="toolcard-result"]'), "collapsed exec card hides result body");
    const expandDetails = commandCard.querySelector<HTMLButtonElement>('[aria-label="Expand tool details"]');
    assert(expandDetails, "command card should expose the explicit details control");
    await act(async () => {
      expandDetails.dispatchEvent(new windowAny.MouseEvent("click", { bubbles: true }));
    });
    await waitForEffects();
    assert(!textOf(host).includes("cwd /repo/app"), "expanded exec card should honor global command visibility");
    assertIncludes(host, "Tests passed", "expanded exec card result");
    const execCard = host.querySelector('[data-testid="chat-tool-call"]');
    assert(execCard, "expected exec card root");
    const execCardText = execCard.textContent ?? "";
    assert(!/\b(pending|running|result)\b/i.test(execCardText), "exec card should not show phase badge");
    assert(
      !buttons(host).some((button) => (button.textContent ?? "").includes("Exit code: 0")),
      "paired tool_result should not render as a separate result button",
    );
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("shell transcript exposes stable tool-call markers while hiding attached result rows", async () => {
  const { host, root } = await renderMessages([
    {
      id: "tool-user-1",
      provider: "test",
      type: "user",
      content: "run a command",
      timestamp: "1700003000001",
    },
    {
      id: "tool-call-1",
      provider: "test",
      type: "tool_call",
      content: "exec",
      timestamp: "1700003000002",
      toolName: "shell",
      toolInput: { command: "echo ok" },
      toolId: "call-1",
      toolKind: "command",
      toolSummary: "echo ok",
      toolDetail: "echo ok",
    },
    {
      id: "tool-result-1",
      provider: "test",
      type: "tool_result",
      content: "ok",
      timestamp: "1700003000003",
      toolId: "call-1",
      toolResult: "ok",
    },
    {
      id: "tool-call-failed-old",
      provider: "test",
      type: "tool_call",
      content: "old failure",
      timestamp: "1700003000004",
      toolName: "shell",
      toolInput: { command: "exit 1" },
      toolId: "call-old",
      toolError: true,
      toolKind: "command",
      toolSummary: "exit 1",
      toolDetail: "exit 1",
    },
    {
      id: "tool-call-failed-latest",
      provider: "test",
      type: "tool_call",
      content: "latest failure",
      timestamp: "1700003000005",
      toolName: "shell",
      toolInput: { command: "exit 2" },
      toolId: "call-latest",
      toolError: true,
      toolKind: "command",
      toolSummary: "exit 2",
      toolDetail: "exit 2",
    },
  ], "tool-call-failed-latest");
  try {
    assert(host.querySelectorAll('[data-testid="chat-tool-call"]').length === 3, "expected one stable marker per shell tool_call");
    assert(host.querySelectorAll('[data-testid="chat-tool-call"]').length === 3, "shell tools render as command cards");
    assertIncludes(host, "run a command", "user message");
    assertIncludes(host, "ok", "attached tool result preview");
    assertIncludes(host, "exit 2", "latest failed command summary");
    assert(!host.querySelector('[data-message-role="tool_result"]'), "attached tool_result should not render as a separate row");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("chatBackend curated tool cards hide internal lifecycle phase badges", async () => {
  const { host, root } = await renderMessages([
    {
      id: "cb-pending-exec",
      provider: "lmstudio",
      type: "tool_call",
      content: "",
      toolName: "exec",
      toolId: "pending-call",
      toolInput: { cmd: "sleep 1" },
      toolArgs: JSON.stringify({ cmd: "sleep 1" }),
      toolKind: "command",
      toolSummary: "sleep 1",
      toolDetail: "sleep 1",
    },
    {
      id: "cb-running-read",
      provider: "lmstudio",
      type: "tool_call",
      content: "",
      toolName: "read_file",
      toolId: "running-call",
      toolInput: { path: "src/status.ts" },
      toolArgs: JSON.stringify({ path: "src/status.ts" }),
      toolKind: "read",
      toolSummary: "src/status.ts",
      toolDetail: "src/status.ts",
    },
    {
      id: "cb-result-write",
      provider: "lmstudio",
      type: "tool_call",
      content: "",
      toolName: "write_file",
      toolId: "result-call",
      toolInput: { path: "src/done.ts", content: "ok" },
      toolArgs: JSON.stringify({ path: "src/done.ts" }),
      toolResult: "written",
      toolKind: "edit",
      toolSummary: "src/done.ts",
      toolDetail: "src/done.ts",
      toolEdits: [{ path: "src/done.ts", old: "", new: "ok" }],
    },
  ]);
  try {
    const cards = Array.from(host.querySelectorAll('[data-testid="chat-tool-call"]'));
    assert(cards.length === 3, `expected three tool cards, got ${cards.length}`);
    for (const card of cards) {
      const label = card.getAttribute("data-testid") ?? "card";
      const text = card.textContent ?? "";
      assert(!/\b(pending|running|result)\b/i.test(text), `${label} should not expose lifecycle phase badge`);
    }
    assertIncludes(host, "sleep 1", "pending exec still shows command");
    assertIncludes(host, "Read", "read_file uses normalized read card");
    assertIncludes(host, "status.ts", "running read still shows file name");
    assert(!textOf(host).includes("written"), "collapsed edit hides attached output");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("failed tool card uses dashed neutral border and muted glyph", async () => {
  const { host, root } = await renderMessages([
    {
      id: "failed-exec",
      provider: "lmstudio",
      type: "tool_call",
      content: "",
      toolName: "exec",
      toolId: "failed-call",
      toolInput: { cmd: "false" },
      toolArgs: JSON.stringify({ cmd: "false" }),
      toolResult: "error: command failed",
      toolError: true,
      toolKind: "command",
      toolSummary: "false",
      toolDetail: "false",
    },
  ]);
  try {
    const card = host.querySelector('[data-testid="chat-tool-call"]') as HTMLElement | null;
    assert(card, "expected failed exec card");
    const style = card!.getAttribute("style") ?? "";
    assert(style.includes("border-style: dashed"), "failed card should be dashed");
    assert(!style.includes("ct-red"), "failed card border should not use red token");
    const failedGlyph = card!.querySelector('[data-testid="toolcard-status"][aria-label="error"] svg');
    assert(failedGlyph, "expected failed glyph");
    assert(!failedGlyph!.getAttribute("class")?.includes("text-red"), "failed glyph should not use red token");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("codex codeterm command after cd chain renders CodeTerm card", async () => {
  const { host, root } = await renderMessages([
    {
      id: "codex-codeterm-chain",
      provider: "codex",
      type: "tool_call",
      content: "",
      toolName: "exec_command",
      toolId: "call-codeterm",
      toolInput: { cmd: "cd frontend && codeterm agent reports" },
      toolArgs: "cd frontend && codeterm agent reports",
      toolKind: "codeterm",
      toolSummary: "agent reports",
      toolDetail: "cd frontend && codeterm agent reports",
    },
    {
      id: "codex-codeterm-chain-result",
      provider: "codex",
      type: "tool_result",
      content: "OK\n",
      toolResult: "OK\n",
      toolId: "call-codeterm",
    },
  ]);
  try {
    assertIncludes(host, "reports", "CodeTerm action label");
    assert(!textOf(host).includes("cd frontend && codeterm"), "CodeTerm card should hide the raw command");
    assert(host.querySelector('[data-testid="chat-tool-call"]'), "codeterm command should render as a tool card");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("standalone arrow-only tool result is hidden", async () => {
  const { host, root } = await renderMessages([
    {
      id: "arrow-only-result",
      provider: "codex",
      type: "tool_result",
      content: "→\n\n->\n⎿",
    },
  ]);
  try {
    assert(textOf(host).trim() === "", `arrow-only tool result should be hidden, got: ${textOf(host)}`);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

const PNG_1X1_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";

test("generate image tool result renders preview instead of raw base64", async () => {
  const dataUrl = `data:image/png;base64,${PNG_1X1_BASE64}`;
  const { host, root } = await renderMessages([
    {
      id: "image-tool-call",
      provider: "cursor",
      type: "tool_call",
      content: "",
      toolName: "GenerateImage",
      toolId: "call-image",
      toolArgs: JSON.stringify({ description: "A red dot" }),
    },
    {
      id: "image-tool-result",
      provider: "cursor",
      type: "tool_result",
      content: JSON.stringify([{ type: "image_url", image_url: { url: dataUrl } }]),
      toolResult: JSON.stringify([{ type: "image_url", image_url: { url: dataUrl } }]),
      toolId: "call-image",
    },
  ]);
  try {
    assert(!host.querySelector('[data-testid="chat-tool-image-preview"]'), "image tool output should start collapsed");
    const expand = host.querySelector<HTMLButtonElement>('[aria-label="Expand tool details"]');
    assert(expand, "image tool output should expose details control");
    await act(async () => {
      expand.dispatchEvent(new windowAny.MouseEvent("click", { bubbles: true }));
    });
    await waitForEffects();
    assert(host.querySelector('[data-testid="chat-tool-image-preview"]'), "expected image preview card");
    const img = host.querySelector('[data-testid="chat-tool-image-preview"] img') as HTMLImageElement | null;
    assert(img, "expected preview img");
    assert(img.src.startsWith("data:image/png;base64,"), "expected data url src");
    assertIncludes(host, "Save", "expected save affordance");
    assert(!textOf(host).includes(PNG_1X1_BASE64.slice(0, 40)), "raw base64 should not render as text");
    assert(host.querySelector('[data-testid="chat-tool-image-download"]'), "expected download link");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("generate image standalone tool_result renders preview card", async () => {
  const dataUrl = `data:image/png;base64,${PNG_1X1_BASE64}`;
  const { host, root } = await renderMessages([
    {
      id: "image-only-result",
      provider: "cursor",
      type: "tool_result",
      content: dataUrl,
      toolResult: dataUrl,
    },
  ]);
  try {
    assert(host.querySelector('[data-testid="chat-tool-image-preview"]'), "expected standalone image preview");
    assertIncludes(host, "PNG", "expected format label");
    assert(!textOf(host).includes(PNG_1X1_BASE64.slice(0, 40)), "standalone result must not leak base64 text");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("codex sparse tool_call rows between exec commands do not render ghost buttons", async () => {
  const { host, root } = await renderMessages([
    {
      id: "codex-exec-1",
      provider: "codex",
      type: "tool_call",
      content: "",
      toolName: "exec_command",
      toolId: "call-1",
      toolInput: { cmd: "npm test" },
      toolArgs: "npm test",
      toolKind: "command",
      toolSummary: "npm test",
      toolDetail: "npm test",
    },
    {
      id: "codex-ghost-1",
      provider: "codex",
      type: "tool_call",
      content: "",
    },
    {
      id: "codex-empty-result",
      provider: "codex",
      type: "tool_result",
      content: "",
    },
    {
      id: "codex-exec-2",
      provider: "codex",
      type: "tool_call",
      content: "",
      toolName: "exec_command",
      toolId: "call-2",
      toolInput: { cmd: "npm run build" },
      toolArgs: "npm run build",
      toolKind: "command",
      toolSummary: "npm run build",
      toolDetail: "npm run build",
    },
  ]);
  try {
    assertIncludes(host, "npm test", "first codex exec command");
    assertIncludes(host, "npm run build", "second codex exec command");
    const toolButtons = Array.from(host.querySelectorAll('[data-testid="chat-tool-call"]'));
    assert(
      toolButtons.length === 2,
      `only real codex exec rows should render shell cards (${toolButtons.length}: ${toolButtons.map((button) => button.textContent ?? "").join(" | ")})`,
    );
    assert(
      toolButtons.every((button) => (button.textContent ?? "").trim()),
      "sparse codex rows should not render chevron-only buttons",
    );
    assert(
      toolButtons.every((button) => (button.textContent ?? "").includes("command")),
      "codex exec_command rows should render as command cards",
    );
    assert(
      !textOf(host).includes("exec_command"),
      "raw exec_command label should not appear in chat UI",
    );
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("chatBackend read_file and write_file render file cards", async () => {
  const { host, root } = await renderMessages([
    {
      id: "cb-read-file",
      provider: "lmstudio",
      type: "tool_call",
      content: "",
      toolName: "read_file",
      toolId: "read-1",
      toolInput: { path: "src/main.ts" },
      toolArgs: JSON.stringify({ path: "src/main.ts" }),
      toolKind: "read",
      toolSummary: "src/main.ts",
      toolDetail: "src/main.ts",
    },
    {
      id: "cb-write-file",
      provider: "lmstudio",
      type: "tool_call",
      content: "",
      toolName: "write_file",
      toolId: "write-1",
      toolInput: { path: "src/out.ts", content: "export const ready = true;\n" },
      toolArgs: JSON.stringify({ path: "src/out.ts" }),
      toolKind: "edit",
      toolSummary: "src/out.ts",
      toolDetail: "src/out.ts",
      toolEdits: [{ path: "src/out.ts", old: "", new: "export const ready = true;\n" }],
    },
  ]);
  try {
    assert(host.querySelectorAll('[data-testid="chat-tool-call"]').length === 2, "expected read and edit cards");
    assertIncludes(host, "main.ts", "read_file name");
    assertIncludes(host, "src/out.ts", "write_file path");
    assertIncludes(host, "export const ready = true;", "write_file preview");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("read_file renders inline row with pending muted spinner", async () => {
  const { host, root } = await renderMessages([
    {
      id: "inline-read",
      provider: "lmstudio",
      type: "tool_call",
      content: "",
      toolName: "read_file",
      toolId: "read-inline-1",
      toolInput: { path: "/tmp/example.ts" },
      toolArgs: JSON.stringify({ path: "/tmp/example.ts" }),
      toolKind: "read",
      toolSummary: "/tmp/example.ts",
      toolDetail: "/tmp/example.ts",
    },
  ]);
  try {
    const inline = host.querySelector('[data-testid="chat-tool-call"]');
    assert(inline, "read_file should render read tool card");
    assert(inline.querySelector('[data-testid="toolcard-status"][aria-label="pending"] svg'), "pending spinner glyph");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("chatBackend unknown tool falls back gracefully", async () => {
  const { host, root } = await renderMessages([
    {
      id: "cb-unknown-tool",
      provider: "lmstudio",
      type: "tool_call",
      content: "mystery_tool({\"value\":1})",
      toolName: "mystery_tool",
      toolId: "unknown-1",
      toolInput: { value: 1 },
      toolArgs: JSON.stringify({ value: 1 }),
      toolKind: "generic",
      toolSummary: "mystery tool",
      toolDetail: "mystery tool",
    },
  ]);
  try {
    assert(host.querySelector('[data-testid="chat-tool-call"]'), "unknown tool should use generic card");
    assertToolCard(host, "mystery tool");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("chatBackend structured inbound renders an expanded typed message card", async () => {
  const { host, root } = await renderMessages([
    {
      id: "inbound-tab-1",
      provider: "lmstudio",
      type: "user",
      content: "Please inspect the last run.",
      fromLabel: "Worker A",
      fromTab: "abc12345",
      fromModel: "gpt-5",
      inboundKind: "tab",
    },
  ]);
  try {
    assertIncludes(host, "from Worker A", "structured inbound sender");
    assertIncludes(host, "gpt-5", "structured inbound model");
    assertIncludes(host, "tab", "structured inbound channel");
    assertIncludes(host, "Please inspect the last run.", "expanded inbound body");
    assert(host.querySelector('[data-testid="chat-inbound-message"]'), "expected typed inbound message card");
    assert(!host.querySelector(`[data-testid="${CROSS_TAB_BUBBLE_TESTID}"]`), "inbound message must not use legacy bubble");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("structured report uses compact typed card instead of a nested message bubble", async () => {
  const content = "task: pl_d17yp2/task_2iw42e\nstatus: In Review\nbranch: lane/tool-cards\nsha: 1e096cba6\ntests: 11/11 green\nSTATUS: in_review | CHANGES: Added provider matrix";
  const descriptor = parseInboundReport(content);
  assert(descriptor.task === "pl_d17yp2/task_2iw42e", "report task parse regression");
  assert(descriptor.status === "in_review", "latest report status should win");
  assert(descriptor.fields.some((field) => field.label === "changes" && field.value === "Added provider matrix"), "compound report fields parse regression");

  const { host, root } = await renderMessages([{
    id: "inbound-report-1",
    provider: "lmstudio",
    type: "user",
    content,
    fromLabel: "Hugo",
    fromTab: "e7f14bec",
    fromModel: "opus",
    inboundKind: "report",
  }]);
  try {
    assert(host.querySelector('[data-testid="chat-inbound-report"]'), "expected typed inbound report card");
    assertIncludes(host, "Report from Hugo", "report sender");
    assertIncludes(host, "task_2iw42e", "report task summary");
    assertIncludes(host, "1e096cba6", "report sha chip");
    assert(!host.querySelector(`[data-testid="${CROSS_TAB_BUBBLE_TESTID}"]`), "report must not use nested cross-tab bubble");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("chatBackend structured mesh inbound renders peer in sender header", async () => {
  const { host, root } = await renderMessages([
    {
      id: "inbound-mesh-1",
      provider: "lmstudio",
      type: "user",
      content: "Remote status is green.",
      fromLabel: "Edison",
      fromTab: "mesh1234",
      fromModel: "sonnet",
      fromMesh: "BLACKSTAR",
      inboundKind: "mesh",
    },
  ]);
  try {
    assertIncludes(host, "Received from Edison @ BLACKSTAR", "structured mesh sender");
    assertIncludes(host, "sonnet", "structured mesh model");
    assertIncludes(host, "mesh", "structured mesh channel");
    assertIncludes(host, "Remote status is green.", "expanded mesh body");
    assert(host.querySelector('[data-testid="chat-inbound-message"]'), "expected typed mesh message card");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("normal user message does not render inbound sender affordance", async () => {
  const { host, root } = await renderMessages([
    {
      id: "normal-user-1",
      provider: "lmstudio",
      type: "user",
      content: "Plain local prompt",
    },
  ]);
  try {
    assertIncludes(host, "Plain local prompt", "normal user content");
    assert(!host.querySelector('[data-testid="chat-inbound-message"]'), "normal user should not have inbound card");
    assert(!textOf(host).includes("from Plain"), "normal user should not have from header");
    assert(host.querySelector(".justify-end"), "normal user should keep right alignment");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

void run();
