import { Window } from "happy-dom";
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";

import { MessageBubble } from "./MessageBubble";
import { reduceToolEvents } from "@codeterm/chat-core";
import {
  type ProviderTranscriptFixture,
} from "./providerTranscriptFixtures.testdata";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { ChatOrigin } from "@codeterm/chat-core";

type TestCase = {
  name: string;
  run: () => void | Promise<void>;
};

/// One registry per spec file. `run` exits non-zero on the first failure AND on an
/// empty registry, so a split that loses a file reports a failure rather than silence.
export function createSuite(label: string) {
  const tests: TestCase[] = [];
  const test = (name: string, run: TestCase["run"]) => {
    tests.push({ name, run });
  };
  const run = async () => {
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
    if (tests.length === 0) {
      console.error(`not ok - ${label} registered no tests`);
      process.exitCode = 1;
      return;
    }
    const elapsedMs = Math.round(performance.now() - startedAt);
    console.log(`ok - ${passed} ${label} tests passed in ${elapsedMs}ms`);
  };
  return { test, run };
}



export function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message);
  }
}

const windowInstance = new Window({
  url: "http://localhost/chat-ui-provider-tests",
});
export const windowAny = windowInstance as unknown as Window & typeof globalThis & {
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

export const clipboardWrites: string[] = [];
Object.defineProperty(windowAny.navigator, "clipboard", {
  configurable: true,
  value: {
    writeText: async (text: string) => {
      clipboardWrites.push(text);
    },
  },
});

export const mermaidSources: string[] = [];
windowAny.mermaid = {
  initialize: () => undefined,
  render: async (_id: string, source: string) => {
    mermaidSources.push(source);
    return {
      svg: `<svg data-testid="mock-mermaid" data-source="${source.replace(/"/g, "&quot;")}"></svg>`,
    };
  },
};

export async function waitForEffects() {
  await act(async () => {
    await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 0));
    await Promise.resolve();
  });
}

export async function renderTranscript(fixture: ProviderTranscriptFixture): Promise<{
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

export async function renderMessages(
  messages: NormalizedChatMessage[],
  latestFailedToolCallId?: string,
  originOptions?: { origin?: ChatOrigin; originAvatar?: ReactNode },
): Promise<{
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
              origin={originOptions?.origin}
              originAvatar={originOptions?.originAvatar}
            />
          );
        })}
      </div>,
    );
  });
  await waitForEffects();
  return { host, root };
}

export function textOf(host: HTMLElement): string {
  return host.textContent ?? "";
}

export function buttons(host: HTMLElement): HTMLButtonElement[] {
  return Array.from(host.querySelectorAll("button"));
}

export function assertIncludes(host: HTMLElement, needle: string, label: string) {
  assert(textOf(host).includes(needle), `${label} should include ${needle}`);
}

export function assertLink(host: HTMLElement, href: string) {
  const link = host.querySelector(`a[href="${href}"]`);
  assert(link, `expected rendered markdown link ${href}`);
}

export function toolCall(
  id: string,
  toolKind: Exclude<NormalizedChatMessage["toolKind"], undefined>,
  overrides: Partial<NormalizedChatMessage> = {},
): NormalizedChatMessage {
  return {
    id,
    provider: "test-provider",
    type: "tool_call",
    content: "",
    toolName: "operation",
    toolId: `${id}-call`,
    toolKind,
    toolSummary: id,
    ...overrides,
  };
}

export function toolCardFor(host: HTMLElement, label: string): HTMLElement | undefined {
  return Array.from(host.querySelectorAll<HTMLElement>('[data-testid="chat-tool-call"]'))
    .find((card) => (card.textContent ?? "").includes(label));
}

export function assertToolCard(host: HTMLElement, label: string) {
  const match = toolCardFor(host, label);
  assert(match, `expected tool card for ${label}`);
  assert(match.querySelector("svg"), `expected tool glyph for ${label}`);
  assert((match.textContent ?? "").includes(label), `expected tool label for ${label}`);
}

export async function expandToolCard(host: HTMLElement, label: string) {
  const card = toolCardFor(host, label);
  assert(card, `expected tool card for ${label}`);
  const expand = card.querySelector<HTMLButtonElement>('[aria-label="Expand tool details"], [aria-label="Show full command"]');
  assert(expand, `expected explicit details control for ${label}`);
  await act(async () => {
    expand.dispatchEvent(new windowAny.MouseEvent("click", { bubbles: true }) as unknown as Event);
  });
  await waitForEffects();
}

export async function clickButtonContaining(host: HTMLElement, label: string) {
  const match = buttons(host).find((button) => (button.textContent ?? "").includes(label));
  assert(match, `expected button containing ${label}`);
  await act(async () => {
    match.dispatchEvent(new windowAny.MouseEvent("click", { bubbles: true }) as unknown as Event);
  });
  await waitForEffects();
}
