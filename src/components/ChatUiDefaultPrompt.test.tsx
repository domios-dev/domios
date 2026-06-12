// Run: npx tsx src/components/ChatUiDefaultPrompt.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { ChatUiDefaultPrompt } from "./ChatUiDefaultPrompt";
import {
  CHAT_UI_DEFAULT_PROMPTED_KEY,
  __setChatUiDefaultConfigWriterForTest,
  maybeAskDefaultNewAgentPaneToChat,
} from "../lib/chatUiMode";

type TestFn = () => void | Promise<void>;

const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "localStorage", { value: window.localStorage, configurable: true });
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

function buttonByText(host: HTMLElement, text: string): HTMLButtonElement {
  const button = Array.from(host.querySelectorAll("button"))
    .find((node) => node.textContent === text);
  assert(button instanceof HTMLButtonElement, `button '${text}' should render`);
  return button;
}

setupDom();

test("renders and yes path writes config", async () => {
  localStorage.clear();
  const writes: boolean[] = [];
  __setChatUiDefaultConfigWriterForTest(async (enabled) => {
    writes.push(enabled);
  });
  const { host, root } = await render(<ChatUiDefaultPrompt />);

  let prompt: Promise<void> | undefined;
  await act(async () => {
    prompt = maybeAskDefaultNewAgentPaneToChat();
  });

  assert(host.querySelector('[role="dialog"]'), "dialog should render");
  assert(host.textContent?.includes("Use Chat UI for agent panes?"), "dialog title should render");

  await act(async () => {
    buttonByText(host, "Use Chat UI").click();
    await prompt;
  });

  assert(writes.length === 1 && writes[0] === true, "yes should write true to config");
  assert(localStorage.getItem(CHAT_UI_DEFAULT_PROMPTED_KEY) === "true", "yes should mark prompted");
  assert(!host.querySelector('[role="dialog"]'), "dialog should close");

  __setChatUiDefaultConfigWriterForTest(null);
  await act(async () => root.unmount());
});

test("no path marks prompted only", async () => {
  localStorage.clear();
  const writes: boolean[] = [];
  __setChatUiDefaultConfigWriterForTest(async (enabled) => {
    writes.push(enabled);
  });
  const { host, root } = await render(<ChatUiDefaultPrompt />);

  let prompt: Promise<void> | undefined;
  await act(async () => {
    prompt = maybeAskDefaultNewAgentPaneToChat();
  });

  await act(async () => {
    buttonByText(host, "Not now").click();
    await prompt;
  });

  assert(writes.length === 0, "no should not write config");
  assert(localStorage.getItem(CHAT_UI_DEFAULT_PROMPTED_KEY) === "true", "no should mark prompted");
  assert(!host.querySelector('[role="dialog"]'), "dialog should close");

  __setChatUiDefaultConfigWriterForTest(null);
  await act(async () => root.unmount());
});

async function main() {
  const started = performance.now();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  const elapsedMs = performance.now() - started;
  console.log(`ChatUiDefaultPrompt tests: ${tests.length - failed}/${tests.length} passed in ${elapsedMs.toFixed(1)}ms`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
