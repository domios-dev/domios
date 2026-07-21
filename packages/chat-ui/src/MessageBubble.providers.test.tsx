import { Window } from "happy-dom";
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";

import { MessageBubble } from "./MessageBubble";
import { parseInboundReport } from "./codeterm/InboundReportCard";
import { CROSS_PANE_BUBBLE_TESTID } from "./crossPaneBubble";
import { reduceToolEvents } from "@codeterm/chat-core";
import {
  providerTranscriptFixtures,
  codexStructuredEditMessages,
  waitProviderMessages,
  type ProviderTranscriptFixture,
} from "./providerTranscriptFixtures.testdata";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

type TestCase = {
  name: string;
  run: () => void | Promise<void>;
};

const tests: TestCase[] = [];

function test(name: string, run: TestCase["run"]) {
  tests.push({ name, run });
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

const windowInstance = new Window({
  url: "http://localhost/chat-ui-provider-tests",
});
const windowAny = windowInstance as unknown as Window & typeof globalThis & {
  mermaid?: {
    initialize: () => void;
    render: (id: string, source: string) => Promise<{ svg: string; bindFunctions?: () => void }>;
  };
};

(globalThis as any).window = windowAny;
(globalThis as any).document = windowAny.document;
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: windowAny.navigator,
});
(globalThis as any).HTMLElement = windowAny.HTMLElement;
(globalThis as any).HTMLButtonElement = windowAny.HTMLButtonElement;
(globalThis as any).HTMLPreElement = windowAny.HTMLPreElement;
(globalThis as any).Node = windowAny.Node;
(globalThis as any).MouseEvent = windowAny.MouseEvent;
(globalThis as any).requestAnimationFrame = (callback: FrameRequestCallback) =>
  setTimeout(() => callback(Date.now()), 0);
(globalThis as any).cancelAnimationFrame = (id: number) => clearTimeout(id);
(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;

const clipboardWrites: string[] = [];
Object.defineProperty(windowAny.navigator, "clipboard", {
  configurable: true,
  value: {
    writeText: async (text: string) => {
      clipboardWrites.push(text);
    },
  },
});

const mermaidSources: string[] = [];
windowAny.mermaid = {
  initialize: () => undefined,
  render: async (_id: string, source: string) => {
    mermaidSources.push(source);
    return {
      svg: `<svg data-testid="mock-mermaid" data-source="${source.replace(/"/g, "&quot;")}"></svg>`,
    };
  },
};

async function waitForEffects() {
  await act(async () => {
    await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 0));
    await Promise.resolve();
  });
}

async function renderTranscript(fixture: ProviderTranscriptFixture): Promise<{
  host: HTMLElement;
  root: Root;
}> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <div>
        {reduceToolEvents(fixture.messages).map((item) => {
          const message = item.kind === "message" ? item.message : item.event.message;
          return <MessageBubble key={message.id} message={message} item={item.kind === "tool" ? item : undefined} />;
        })}
      </div>,
    );
  });
  await waitForEffects();
  return { host, root };
}

async function renderMessages(messages: NormalizedChatMessage[], latestFailedToolCallId?: string): Promise<{
  host: HTMLElement;
  root: Root;
}> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <div>
        {reduceToolEvents(messages).map((item) => {
          const message = item.kind === "message" ? item.message : item.event.message;
          return (
            <MessageBubble
              key={message.id}
              message={message}
              item={item.kind === "tool" ? item : undefined}
              isLatestFailedToolCall={message.id === latestFailedToolCallId}
            />
          );
        })}
      </div>,
    );
  });
  await waitForEffects();
  return { host, root };
}

function textOf(host: HTMLElement): string {
  return host.textContent ?? "";
}

function buttons(host: HTMLElement): HTMLButtonElement[] {
  return Array.from(host.querySelectorAll("button"));
}

function assertIncludes(host: HTMLElement, needle: string, label: string) {
  assert(textOf(host).includes(needle), `${label} should include ${needle}`);
}

function assertLink(host: HTMLElement, href: string) {
  const link = host.querySelector(`a[href="${href}"]`);
  assert(link, `expected rendered markdown link ${href}`);
}

function toolCardFor(host: HTMLElement, label: string): HTMLElement | undefined {
  return Array.from(host.querySelectorAll<HTMLElement>('[data-testid="chat-tool-call"]'))
    .find((card) => (card.textContent ?? "").includes(label));
}

function assertToolCard(host: HTMLElement, label: string) {
  const match = toolCardFor(host, label);
  assert(match, `expected tool card for ${label}`);
  assert(match.querySelector("svg"), `expected tool glyph for ${label}`);
  assert((match.textContent ?? "").includes(label), `expected tool label for ${label}`);
}

async function expandToolCard(host: HTMLElement, label: string) {
  const card = toolCardFor(host, label);
  assert(card, `expected tool card for ${label}`);
  const expand = card.querySelector<HTMLButtonElement>('[aria-label="Expand tool details"]');
  assert(expand, `expected explicit details control for ${label}`);
  await act(async () => {
    expand.dispatchEvent(new windowAny.MouseEvent("click", { bubbles: true }));
  });
  await waitForEffects();
}

async function clickButtonContaining(host: HTMLElement, label: string) {
  const match = buttons(host).find((button) => (button.textContent ?? "").includes(label));
  assert(match, `expected button containing ${label}`);
  await act(async () => {
    match.dispatchEvent(new windowAny.MouseEvent("click", { bubbles: true }));
  });
  await waitForEffects();
}

for (const fixture of providerTranscriptFixtures) {
  test(`${fixture.provider} renders normalized provider transcript branches`, async () => {
    mermaidSources.length = 0;
    clipboardWrites.length = 0;

    const { host, root } = await renderTranscript(fixture);
    try {
      assertIncludes(host, fixture.expected.attachmentName, `${fixture.provider} attachment`);
      assertIncludes(host, fixture.expected.systemLabel, `${fixture.provider} system subtype`);
      assertIncludes(host, "Thinking", `${fixture.provider} thinking summary`);
      assert(host.querySelector("table"), `${fixture.provider} should render markdown table`);
      assertLink(host, fixture.expected.linkHref);
      assertIncludes(host, fixture.expected.codeNeedle, `${fixture.provider} fenced code`);
      assert(
        host.querySelector('[data-testid="mock-mermaid"]'),
        `${fixture.provider} should render mocked mermaid output`,
      );
      assert(
        mermaidSources.some((source) => source.includes(fixture.expected.mermaidNeedle)),
        `${fixture.provider} should send mermaid source to renderer`,
      );
      for (const label of fixture.expected.toolLabels) {
        assertToolCard(host, label);
      }
      await expandToolCard(host, fixture.expected.toolLabels[0]);
      assertIncludes(host, fixture.expected.resultNeedle, `${fixture.provider} tool result`);
      if (fixture.expected.diffNeedle) {
        assertIncludes(host, fixture.expected.diffNeedle, `${fixture.provider} structured edit added line`);
      }
      if (fixture.expected.diffRemovedNeedle) {
        assertIncludes(host, fixture.expected.diffRemovedNeedle, `${fixture.provider} structured edit removed line`);
      }

      await clickButtonContaining(host, "Thinking");
      assertIncludes(host, fixture.expected.thinkingNeedle, `${fixture.provider} expanded thinking`);

      await clickButtonContaining(host, "Copy");
      assert(
        clipboardWrites.some((write) => write.includes(fixture.expected.codeNeedle)),
        `${fixture.provider} copy button should write fenced code`,
      );
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  test(`${fixture.provider} mixed transcript structure stays stable`, async () => {
    const { host, root } = await renderTranscript(fixture);
    try {
      const copyButtonCount = buttons(host).filter((button) => (button.textContent ?? "").includes("Copy")).length;
      const toolCardCount = fixture.expected.toolLabels.filter((label) =>
        toolCardFor(host, label) != null,
      ).length;
      const counts = {
        messageFixtures: fixture.messages.length,
        tables: host.querySelectorAll("table").length,
        links: host.querySelectorAll("a").length,
        mermaid: host.querySelectorAll('[data-testid="mock-mermaid"]').length,
        toolCards: toolCardCount,
        copyButtons: copyButtonCount,
        attachmentMentions: textOf(host).includes(fixture.expected.attachmentName) ? 1 : 0,
      };

      assert(counts.messageFixtures >= 6, `${fixture.provider} fixture should remain mixed`);
      assert(counts.tables === 1, `${fixture.provider} should render one markdown table`);
      assert(counts.links >= 1, `${fixture.provider} should render markdown links`);
      assert(counts.mermaid === 1, `${fixture.provider} should render one mermaid block`);
      assert(counts.toolCards === fixture.expected.toolLabels.length, `${fixture.provider} should render all tool cards`);
      assert(counts.copyButtons >= 1, `${fixture.provider} should expose copy interactions`);
      assert(counts.attachmentMentions >= 1, `${fixture.provider} should render attachment context`);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
}

test("wait batches are accessible, lossless, and provider-neutral", async () => {
  for (const [provider, messages] of Object.entries(waitProviderMessages)) {
    const item = reduceToolEvents(messages)[0];
    assert(item.kind === "tool" && item.event.waitBatch, `${provider} normalized casing reaches the wait predicate`);
  }
  const messages: NormalizedChatMessage[] = [
    { id: "wait-one", type: "tool_call", content: "", toolId: "one", toolName: "wait", toolKind: "generic", toolSummary: "first wait", toolResult: "first result", timestamp: "1000" },
    { id: "wait-two", type: "tool_call", content: "", toolId: "two", toolName: "wait_for", toolKind: "generic", toolSummary: "second wait", toolResult: "second result", timestamp: "2500" },
    { id: "command", type: "tool_call", content: "", toolId: "command", toolName: "wait", toolKind: "command", toolSummary: "command must remain separate" },
    { id: "assistant-break", type: "assistant", content: "breaks the run" },
    { id: "wait-three", type: "tool_call", content: "", toolId: "three", toolName: "tool_wait", toolKind: "generic", toolSummary: "third wait" },
  ];
  const items = reduceToolEvents(messages);
  assert(items.length === 4, "interleaved command and assistant preserve timeline order");
  const { host, root } = await renderMessages(messages);
  try {
    const card = host.querySelector<HTMLElement>('[data-testid="chat-wait-batch"]');
    assert(card, "wait run renders a compact card");
    const button = card.querySelector<HTMLButtonElement>('button[aria-label="Expand 2 wait events"]');
    assert(button, "wait batch uses a named real button");
    assert(button.getAttribute("aria-expanded") === "false", "details start collapsed");
    const controls = button.getAttribute("aria-controls");
    assert(controls && host.querySelector(`#${controls}`)?.hasAttribute("hidden"), "aria-controls references the collapsed details region");
    assertIncludes(card, "2 wait events", "visible count");
    assertIncludes(card, "1.5s", "truthful elapsed summary");
    await act(async () => button.dispatchEvent(new windowAny.KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    await waitForEffects();
    assert(button.getAttribute("aria-expanded") === "true", "keyboard/click activation expands details");
    const region = host.querySelector<HTMLElement>(`#${controls}`);
    assert(region?.getAttribute("role") === "region", "details have an accessible region");
    assertIncludes(region ?? host, "first wait", "first member detail remains visible");
    assertIncludes(region ?? host, "first result", "first member result remains visible");
    assertIncludes(region ?? host, "second wait", "second member detail remains visible");
    assert(!textOf(card).includes("command must remain separate"), "unrelated command does not leak into wait details");
    await act(async () => button.dispatchEvent(new windowAny.KeyboardEvent("keydown", { key: " ", bubbles: true })));
    await waitForEffects();
    assert(button.getAttribute("aria-expanded") === "false", "space activation collapses details");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("Claude compact summary renders as collapsed neutral system artifact", async () => {
  const compactBody = [
    "This session is being continued from a previous conversation.",
    "Summary preview line.",
    "Full compact body visible only after expansion.",
  ].join("\n");
  const { host, root } = await renderMessages([
    {
      id: "compact-summary",
      type: "system",
      systemSubtype: "compact_summary",
      content: compactBody,
      collapsed: true,
    },
    {
      id: "compact-boundary",
      type: "status",
      systemSubtype: "compact_boundary",
      content: "Context compacted",
    },
  ]);
  try {
    assertIncludes(host, "Session continued from summary", "compact summary label");
    assertIncludes(host, "Context compacted", "compact boundary status");
    assert(
      !textOf(host).includes("Full compact body visible only after expansion."),
      "compact summary body should be collapsed by default",
    );

    await clickButtonContaining(host, "Session continued from summary");
    assertIncludes(host, "Full compact body visible only after expansion.", "expanded compact summary body");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("Claude system-shaped user rows render neutral after normalization", async () => {
  const { host, root } = await renderMessages([
    {
      id: "dante-compact-summary",
      type: "system",
      systemSubtype: "compact_summary",
      content: [
        "This session is being continued from a previous conversation that ran out of context.",
        "",
        "Summary:",
        "- I (Dante, Opus) was made the CodeTerm orchestrator.",
      ].join("\n"),
      collapsed: true,
      isCompactSummary: true,
      isVisibleInTranscriptOnly: true,
    },
    {
      id: "dante-task-notification",
      type: "status",
      systemSubtype: "task_notification",
      content: "Task completed",
    },
  ]);
  try {
    assertIncludes(host, "Session continued from summary", "Dante compact summary label");
    assertIncludes(host, "Task completed", "Dante task notification status");
    assert(!textOf(host).includes("<task-notification>"), "raw task notification XML should not render");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("Codex injected transcript rows render as collapsed neutral artifacts", async () => {
  const agentInstructions = [
    "# AGENTS.md instructions for /Users/dev/Developer/codeterm-alpha",
    "",
    "<INSTRUCTIONS>",
    "Full instructions visible only after expansion.",
    "</INSTRUCTIONS>",
  ].join("\n");
  const { host, root } = await renderMessages([
    {
      id: "codex-agent-instructions",
      provider: "codex",
      type: "system",
      systemSubtype: "agent_instructions",
      content: agentInstructions,
      collapsed: true,
    },
    {
      id: "codex-turn-aborted",
      provider: "codex",
      type: "system",
      systemSubtype: "turn_aborted",
      content: "<turn_aborted>\nThe user interrupted the previous turn on purpose.\n</turn_aborted>",
      collapsed: true,
    },
    {
      id: "codex-environment-context",
      provider: "codex",
      type: "system",
      systemSubtype: "environment_context",
      content: "<environment_context>\n  <cwd>D:/Developer/codeterm</cwd>\n</environment_context>",
      collapsed: true,
    },
  ]);
  try {
    assertIncludes(host, "System context", "Codex instructions label");
    assertIncludes(host, "Environment context", "Codex environment label");
    assertIncludes(host, "Turn interrupted", "Codex turn aborted label");
    assert(
      !textOf(host).includes("Full instructions visible only after expansion."),
      "Codex instructions should be collapsed by default",
    );

    await clickButtonContaining(host, "System context");
    assertIncludes(host, "Full instructions visible only after expansion.", "expanded Codex instructions body");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("codex structured apply_patch renders diff from real-session patch text", async () => {
  const { host, root } = await renderMessages(codexStructuredEditMessages);
  try {
    assertIncludes(host, "edit", "structured edit tool label");
    assertIncludes(host, "mem/mod.rs", "structured edit file path");
    assertIncludes(host, "let body = text.trim()", "structured edit added hunk");
    assertIncludes(host, "val.to_string()", "structured edit removed hunk");
    assertIncludes(host, "+", "structured edit shows added line count");
    assertIncludes(host, "-", "structured edit shows removed line count");
    assert(!textOf(host).includes("Success. Updated the following files"), "structured edit hides redundant result output");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("completed text output stays hidden while collapsed", async () => {
  const { host, root } = await renderMessages([{
      id: "compact-result",
      provider: "codex",
      type: "tool_call",
      content: "",
      toolName: "shell_command",
      toolId: "compact-result-call",
      toolInput: { command: "printf output" },
      toolResult: "first line\n" + "x".repeat(400),
      toolKind: "command",
      toolSummary: "printf output",
      toolDetail: "printf output",
    }]);
  try {
    assert(!host.querySelector('[title="Show full output"]'), "collapsed tool card has no result subtitle");
    assert(!textOf(host).includes("first line"), "collapsed tool result text stays hidden");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("codex exec_command rg search does not render as structured edit diff", async () => {
  const { host, root } = await renderMessages([
    codexStructuredEditMessages[0],
  ]);
  try {
    assert(host.querySelector('[data-testid="chat-tool-call"]'), "exec_command should render command card");
    assertIncludes(host, "command", "exec_command card label");
    assertIncludes(host, "rg -n", "rg exec command renders");
    assert(!textOf(host).includes("exec_command"), "raw exec_command label should not appear");
    assert(!textOf(host).includes("let body = text.trim()"), "rg search must not show patch diff content");
    const diffMarkers = textOf(host).match(/\+\d+/g) ?? [];
    assert(diffMarkers.length === 0, "rg search must not show +/- edit counts");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

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
    assertIncludes(host, "read", "read_file uses normalized read card");
    assertIncludes(host, "src/status.ts", "running read still shows path");
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
    assertIncludes(host, "src/main.ts", "read_file path");
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
      id: "inbound-pane-1",
      provider: "lmstudio",
      type: "user",
      content: "Please inspect the last run.",
      fromLabel: "Worker A",
      fromPane: "abc12345",
      fromModel: "gpt-5",
      inboundKind: "pane",
    },
  ]);
  try {
    assertIncludes(host, "from Worker A", "structured inbound sender");
    assertIncludes(host, "gpt-5", "structured inbound model");
    assertIncludes(host, "tab", "structured inbound channel");
    assertIncludes(host, "Please inspect the last run.", "expanded inbound body");
    assert(host.querySelector('[data-testid="chat-inbound-message"]'), "expected typed inbound message card");
    assert(!host.querySelector(`[data-testid="${CROSS_PANE_BUBBLE_TESTID}"]`), "inbound message must not use legacy bubble");
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
    fromPane: "e7f14bec",
    fromModel: "opus",
    inboundKind: "report",
  }]);
  try {
    assert(host.querySelector('[data-testid="chat-inbound-report"]'), "expected typed inbound report card");
    assertIncludes(host, "report · Hugo", "report sender");
    assertIncludes(host, "task_2iw42e", "report task summary");
    assertIncludes(host, "1e096cba6", "report sha chip");
    assert(!host.querySelector(`[data-testid="${CROSS_PANE_BUBBLE_TESTID}"]`), "report must not use nested cross-pane bubble");
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
      fromPane: "mesh1234",
      fromModel: "sonnet",
      fromMesh: "BLACKSTAR",
      inboundKind: "mesh",
    },
  ]);
  try {
    assertIncludes(host, "from Edison@BLACKSTAR", "structured mesh sender");
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

async function run() {
  const startedAt = performance.now();
  let passed = 0;
  for (const entry of tests) {
    try {
      await entry.run();
      passed += 1;
      console.log(`ok - ${entry.name}`);
    } catch (error) {
      console.error(`not ok - ${entry.name}`);
      console.error(error);
      process.exitCode = 1;
      return;
    }
  }
  const elapsedMs = Math.round(performance.now() - startedAt);
  console.log(`ok - ${passed} MessageBubble provider tests passed in ${elapsedMs}ms`);
}

void run();
