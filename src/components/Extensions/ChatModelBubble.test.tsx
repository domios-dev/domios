// Run: npx tsx src/components/Extensions/ChatModelBubble.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { ChatModelBubble } from "./ChatModelBubble";
import {
  __setChatModelBubbleApiForTest,
  type ChatModelBubbleApi,
} from "./chatModelBubbleApi";
import type { ModelInfo } from "../../lib/types";

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
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
  Object.defineProperty(globalThis, "setTimeout", { value: window.setTimeout.bind(window), configurable: true });
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

// Flush mount effects + their chained microtasks.
async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

const MODELS: ModelInfo[] = [
  { id: "qwen-2.5", display_name: "Qwen 2.5" },
  { id: "llama-3", display_name: "Llama 3" },
];

function fakeApi(overrides: Partial<ChatModelBubbleApi> = {}): {
  api: ChatModelBubbleApi;
  switchCalls: Array<{ pluginId: string; sessionId: string; model: string }>;
} {
  const switchCalls: Array<{ pluginId: string; sessionId: string; model: string }> = [];
  const api: ChatModelBubbleApi = {
    sessionInfo: async () => ({ model: "qwen-2.5" }),
    availableModels: async () => MODELS,
    switchModel: async (pluginId, sessionId, model) => {
      switchCalls.push({ pluginId, sessionId, model });
      return { model };
    },
    ...overrides,
  };
  return { api, switchCalls };
}

function bubble(host: HTMLElement): HTMLButtonElement | null {
  return host.querySelector('[data-testid="chat-model-bubble"]');
}

function options(host: HTMLElement): HTMLButtonElement[] {
  return Array.from(host.querySelectorAll('[data-testid="chat-model-option"]'));
}

setupDom();

test("renders the current model from session_info", async () => {
  const { api } = fakeApi();
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="lmstudio" sessionId="s1" />);
  await flush();

  const b = bubble(host);
  assert(b, "bubble should render");
  assert(b.textContent?.includes("Qwen 2.5"), `bubble should show current model display name, got: ${b.textContent}`);

  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("uses the terminal top-right pane bubble slot", async () => {
  const { api } = fakeApi();
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="lmstudio" sessionId="s1" />);
  await flush();

  const b = bubble(host);
  assert(b, "bubble should render");
  assert(b.classList.contains("ct-chat-model-bubble"), "bubble should expose the pane primary bubble class");
  const slot = b.parentElement as HTMLDivElement | null;
  assert(slot, "bubble should have a positioned slot parent");
  assert(slot.style.right === "8px", `bubble slot should use the terminal right edge, got: ${slot.style.right}`);
  assert(slot.style.left === "", `bubble slot should not use the old left edge, got: ${slot.style.left}`);

  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("opens a dropdown listing the available models", async () => {
  const { api } = fakeApi();
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="lmstudio" sessionId="s1" />);
  await flush();

  assert(options(host).length === 0, "dropdown should start closed");
  await act(async () => {
    bubble(host)!.click();
  });

  const opts = options(host);
  assert(opts.length === MODELS.length, `dropdown should list ${MODELS.length} models, got ${opts.length}`);
  assert(opts.some((o) => o.textContent?.includes("Llama 3")), "dropdown should include Llama 3");

  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("picking a model invokes switch_model and updates the bubble", async () => {
  const { api, switchCalls } = fakeApi();
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="lmstudio" sessionId="s1" />);
  await flush();

  await act(async () => {
    bubble(host)!.click();
  });
  const llama = options(host).find((o) => o.getAttribute("data-model-id") === "llama-3");
  assert(llama, "Llama 3 option should render");
  await act(async () => {
    llama.click();
  });
  await flush();

  assert(switchCalls.length === 1, `switch_model should be called once, got ${switchCalls.length}`);
  assert(
    switchCalls[0].pluginId === "lmstudio"
      && switchCalls[0].sessionId === "s1"
      && switchCalls[0].model === "llama-3",
    `switch_model called with wrong args: ${JSON.stringify(switchCalls[0])}`,
  );
  assert(bubble(host)?.textContent?.includes("Llama 3"), "bubble should reflect the newly selected model");
  assert(options(host).length === 0, "dropdown should close after picking");

  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("renders nothing when the backend exposes no switchable models", async () => {
  const { api } = fakeApi({ availableModels: async () => [] });
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="bare" sessionId="s1" />);
  await flush();

  assert(!bubble(host), "no bubble should render without available models");

  __setChatModelBubbleApiForTest(null);
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
  console.log(`ChatModelBubble tests: ${tests.length - failed}/${tests.length} passed in ${elapsedMs.toFixed(1)}ms`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
