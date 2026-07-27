// Run: npx tsx packages/chat-ui/src/MessageBubble.watcher.test.tsx

import { Window } from "happy-dom";
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "./MessageBubble";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];

function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const windowInstance = new Window({ url: "http://localhost/chat-ui-watcher-tests" });
const windowAny = windowInstance as unknown as Window & typeof globalThis;

(globalThis as any).window = windowAny;
(globalThis as any).document = windowAny.document;
Object.defineProperty(globalThis, "navigator", { configurable: true, value: windowAny.navigator });
(globalThis as any).HTMLElement = windowAny.HTMLElement;
(globalThis as any).HTMLButtonElement = windowAny.HTMLButtonElement;
(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as any).requestAnimationFrame = (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 0);
(globalThis as any).cancelAnimationFrame = (id: number) => clearTimeout(id);

async function renderMessage(message: NormalizedChatMessage): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<MessageBubble message={message} />);
  });
  await act(async () => {
    await Promise.resolve();
  });
  return { host, root };
}

function baseMessage(type: NormalizedChatMessage["type"], content: string): NormalizedChatMessage {
  return { id: `m-${type}`, type, content, timestamp: "1" };
}

test("context_request renders collapsed context card by default", async () => {
  const { host, root } = await renderMessage(
    baseMessage("context_request", '{"state":{},"input":{"tick":1}}'),
  );
  assert(host.querySelector('[data-testid="chat-context-request-card"]'), "context card");
  assert(host.textContent?.includes("Context sent"), "context title");
  assert(!host.textContent?.includes('"tick":1'), "payload hidden while collapsed");
  await act(async () => root.unmount());
});

test("watcher_verdict renders status card with actions", async () => {
  const content = JSON.stringify({
    status: "stalled",
    summary: "Orchestrator idle",
    actions: [{ kind: "nudge", tab: "worker-1", message: "Resume task" }],
  });
  const { host, root } = await renderMessage(baseMessage("watcher_verdict", content));
  assert(host.querySelector('[data-testid="chat-watcher-verdict-card"]'), "verdict card");
  assert(host.querySelector('[data-testid="watcher-verdict-status-chip"]')?.textContent === "STALLED", "status chip");
  assert(host.getAttribute("data-verdict-tone") == null, "tone on card element");
  assert(
    host.querySelector('[data-testid="chat-watcher-verdict-card"]')?.getAttribute("data-verdict-tone") === "danger",
    "stalled tone",
  );
  assert(host.textContent?.includes("Orchestrator idle"), "summary");
  await act(async () => root.unmount());
});

test("malformed watcher_verdict degrades to attention chip with raw summary", async () => {
  const raw = "totally not json";
  const { host, root } = await renderMessage(baseMessage("watcher_verdict", raw));
  const card = host.querySelector('[data-testid="chat-watcher-verdict-card"]');
  assert(card, "verdict card");
  assert(card?.getAttribute("data-verdict-tone") === "warn", "malformed uses attention/warn tone");
  assert(host.querySelector('[data-testid="watcher-verdict-status-chip"]')?.textContent === "ATTENTION", "attention chip");
  assert(host.textContent?.includes(raw), "raw summary visible");
  await act(async () => root.unmount());
});

test("unknown status degrades to attention warn chip", async () => {
  const content = JSON.stringify({ status: "exploded", summary: "weird", actions: [] });
  const { host, root } = await renderMessage(baseMessage("watcher_verdict", content));
  assert(
    host.querySelector('[data-testid="chat-watcher-verdict-card"]')?.getAttribute("data-verdict-tone") === "warn",
    "unknown status tone",
  );
  assert(host.textContent?.includes("weird"), "summary preserved");
  await act(async () => root.unmount());
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`PASS ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`FAIL ${name}`, error);
    }
  }
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
