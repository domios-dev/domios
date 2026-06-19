import { Window } from "happy-dom";
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";

import { MessageBubble } from "./MessageBubble";
import { attachToolResults } from "@codeterm/chat-core";
import {
  providerTranscriptFixtures,
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
        {fixture.messages.map((message) => (
          <MessageBubble key={message.id} message={message} />
        ))}
      </div>,
    );
  });
  await waitForEffects();
  return { host, root };
}

async function renderMessages(messages: NormalizedChatMessage[]): Promise<{
  host: HTMLElement;
  root: Root;
}> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <div>
        {attachToolResults(messages).map((message) => (
          <MessageBubble key={message.id} message={message} />
        ))}
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

function assertToolIcon(host: HTMLElement, label: string) {
  const match = buttons(host).find((button) => (button.textContent ?? "").includes(label));
  assert(match, `expected tool button for ${label}`);
  assert(match.querySelector("svg"), `expected toolIcons svg for ${label}`);
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
      assertIncludes(host, fixture.expected.resultNeedle, `${fixture.provider} tool result`);
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
        assertToolIcon(host, label);
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
      const toolButtonCount = fixture.expected.toolLabels.filter((label) =>
        buttons(host).some((button) => (button.textContent ?? "").includes(label)),
      ).length;
      const counts = {
        messageFixtures: fixture.messages.length,
        tables: host.querySelectorAll("table").length,
        links: host.querySelectorAll("a").length,
        mermaid: host.querySelectorAll('[data-testid="mock-mermaid"]').length,
        toolButtons: toolButtonCount,
        copyButtons: copyButtonCount,
        attachmentMentions: textOf(host).includes(fixture.expected.attachmentName) ? 1 : 0,
      };

      assert(counts.messageFixtures >= 6, `${fixture.provider} fixture should remain mixed`);
      assert(counts.tables === 1, `${fixture.provider} should render one markdown table`);
      assert(counts.links >= 1, `${fixture.provider} should render markdown links`);
      assert(counts.mermaid === 1, `${fixture.provider} should render one mermaid block`);
      assert(counts.toolButtons === fixture.expected.toolLabels.length, `${fixture.provider} should render all tool buttons`);
      assert(counts.copyButtons >= 1, `${fixture.provider} should expose copy interactions`);
      assert(counts.attachmentMentions >= 1, `${fixture.provider} should render attachment context`);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
}

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
    assert(host.querySelector('[data-testid="chatbackend-tool-exec"]'), "expected exec card");
    assertIncludes(host, "npm test -- --runInBand", "exec card command");
    assertIncludes(host, "cwd /repo/app", "exec card cwd");
    assertIncludes(host, "result", "exec card phase");
    assertIncludes(host, "Tests passed", "exec card result preview");
    assert(
      !buttons(host).some((button) => (button.textContent ?? "").includes("Exit code: 0")),
      "paired tool_result should not render as a separate result button",
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
    },
  ]);
  try {
    assert(host.querySelector('[data-testid="chatbackend-tool-read_file"]'), "expected read_file card");
    assert(host.querySelector('[data-testid="chatbackend-tool-write_file"]'), "expected write_file card");
    assertIncludes(host, "src/main.ts", "read_file path");
    assertIncludes(host, "src/out.ts", "write_file path");
    assertIncludes(host, "export const ready = true;", "write_file preview");
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
    },
  ]);
  try {
    assert(!host.querySelector('[data-testid^="chatbackend-tool-"]'), "unknown tool should not use curated card");
    assertToolIcon(host, "mystery_tool");
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
