// Run: npx tsx src/components/Extensions/ChatPresetBubble.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { ChatPresetBubble } from "./ChatPresetBubble";
import {
  __setPresetManagerApiForTest,
  type PresetManagerApi,
  type PresetState,
} from "./presetManagerApi";

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

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function fakeApi(initial: PresetState): {
  api: PresetManagerApi;
  saves: PresetState[];
  applies: Array<{ pluginId: string; sessionId: string; model: string }>;
} {
  const saves: PresetState[] = [];
  const applies: Array<{ pluginId: string; sessionId: string; model: string }> = [];
  const api: PresetManagerApi = {
    load: async () => structuredClone(initial),
    save: async (_pluginId, state) => {
      saves.push(structuredClone(state));
    },
    applyToSession: async (pluginId, sessionId, model) => {
      applies.push({ pluginId, sessionId, model });
    },
  };
  return { api, saves, applies };
}

const SEED: PresetState = {
  presets: [
    { id: "p1", name: "Coder", systemPrompt: "You write code.", model: "qwen-2.5" },
    { id: "p2", name: "Writer", systemPrompt: "You write prose." },
  ],
  activePreset: "p1",
};

function bubble(host: HTMLElement): HTMLButtonElement | null {
  return host.querySelector('[data-testid="chat-preset-bubble"]');
}
function options(host: HTMLElement): HTMLButtonElement[] {
  return Array.from(host.querySelectorAll('[data-testid="chat-preset-option"]'));
}

setupDom();

test("renders nothing when the backend has no presets", async () => {
  const { api } = fakeApi({ presets: [], activePreset: null });
  __setPresetManagerApiForTest(api);
  const { host, root } = await render(<ChatPresetBubble pluginId="lmstudio" sessionId="s1" />);
  await flush();

  assert(!bubble(host), "no bubble should render without presets");

  __setPresetManagerApiForTest(null);
  await act(async () => root.unmount());
});

test("shows the active preset name", async () => {
  const { api } = fakeApi(SEED);
  __setPresetManagerApiForTest(api);
  const { host, root } = await render(<ChatPresetBubble pluginId="lmstudio" sessionId="s1" />);
  await flush();

  assert(bubble(host), "bubble should render when presets exist");
  assert(host.textContent?.includes("Coder"), "bubble should show the active preset name");

  __setPresetManagerApiForTest(null);
  await act(async () => root.unmount());
});

test("with a sessionId, picking a preset applies its bound model to the live session and persists the choice", async () => {
  const { api, saves, applies } = fakeApi(SEED);
  __setPresetManagerApiForTest(api);
  const { host, root } = await render(<ChatPresetBubble pluginId="lmstudio" sessionId="s1" />);
  await flush();

  await act(async () => {
    bubble(host)!.click();
  });
  const opts = options(host);
  assert(opts.length === 2, `expected 2 preset options, got ${opts.length}`);
  // Pick Coder (p1) — bound to qwen-2.5.
  await act(async () => {
    host.querySelector<HTMLButtonElement>('[data-testid="chat-preset-option"][data-preset-id="p1"]')!.click();
  });
  await flush();

  assert(applies.length === 1, `should apply once to the session, got ${applies.length}`);
  assert(
    applies[0].pluginId === "lmstudio" && applies[0].sessionId === "s1" && applies[0].model === "qwen-2.5",
    `apply called with wrong args: ${JSON.stringify(applies[0])}`,
  );
  const last = saves[saves.length - 1];
  assert(last?.activePreset === "p1", `activePreset should persist as p1, got ${last?.activePreset}`);

  __setPresetManagerApiForTest(null);
  await act(async () => root.unmount());
});

test("without a sessionId, picking a preset persists the choice but does not apply to a session", async () => {
  const { api, saves, applies } = fakeApi(SEED);
  __setPresetManagerApiForTest(api);
  const { host, root } = await render(<ChatPresetBubble pluginId="lmstudio" />);
  await flush();

  await act(async () => {
    bubble(host)!.click();
  });
  await act(async () => {
    host.querySelector<HTMLButtonElement>('[data-testid="chat-preset-option"][data-preset-id="p1"]')!.click();
  });
  await flush();

  assert(applies.length === 0, `should not apply to any session without a sessionId, got ${applies.length}`);
  const last = saves[saves.length - 1];
  assert(last?.activePreset === "p1", `activePreset should still persist as p1, got ${last?.activePreset}`);

  __setPresetManagerApiForTest(null);
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
  console.log(`ChatPresetBubble tests: ${tests.length - failed}/${tests.length} passed in ${elapsedMs.toFixed(1)}ms`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
