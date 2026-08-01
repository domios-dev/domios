// Run: npx tsx src/components/Extensions/ChatModelBubble.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { ChatModelBubble } from "./ChatModelBubble";
import {
  __setChatModelBubbleApiForTest,
  type ChatModelBubbleApi,
} from "./chatModelBubbleApi";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import type { ModelInfo, ProviderDescriptor } from "../../lib/types";

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
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

const MODELS: ModelInfo[] = [
  { id: "model-a", display_name: "Model A" },
  { id: "model-b", display_name: "Model B" },
];

let catalogModels = MODELS;
let modelsUpdated: (() => void) | undefined;

function catalogProvider(): ProviderDescriptor {
  return {
    schema_version: 1,
    id: "provider-shell",
    display_name: "Provider Shell",
    transport: "chat_backend",
    system_prompt_delivery: "external",
    available_models: catalogModels,
    default_model: catalogModels[0]?.id,
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
  };
}

function fakeApi(overrides: Partial<ChatModelBubbleApi> = {}): {
  api: ChatModelBubbleApi;
  switchCalls: Array<{ pluginId: string; sessionId: string; model: string }>;
} {
  const switchCalls: Array<{ pluginId: string; sessionId: string; model: string }> = [];
  const api: ChatModelBubbleApi = {
    sessionInfo: async () => ({ model: "model-a" }),
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

// it lives inside the bubble's wrapper subtree — query the whole document.
function confirmDialog(): HTMLElement | null {
  return document.querySelector('[data-testid="chat-model-switch-confirm"]');
}

async function pick(host: HTMLElement, modelId: string) {
  await act(async () => {
    bubble(host)!.click();
  });
  const opt = options(host).find((o) => o.getAttribute("data-model-id") === modelId);
  assert(opt, `option ${modelId} should render`);
  await act(async () => {
    opt.click();
  });
  await flush();
}

setupDom();
__setInvokeForTest(async <T,>(command: string): Promise<T> => {
  if (command === "list_agent_providers") return [catalogProvider()] as unknown as T;
  throw new Error(`Unexpected command: ${command}`);
});
__setListenForTest(async <T,>(event: string, handler: (event: { payload: T }) => void) => {
  if (event === "models-updated") {
    modelsUpdated = () => handler({ payload: { provider: "provider-shell" } as T });
  }
  return () => {
    if (event === "models-updated") modelsUpdated = undefined;
  };
});

test("renders the current model from session_info", async () => {
  const { api } = fakeApi();
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="provider-shell" sessionId="s1" />);
  await flush();

  const b = bubble(host);
  assert(b, "bubble should render");
  assert(b.textContent?.includes("Model A"), `bubble should show current model display name, got: ${b.textContent}`);

  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("renders as an inline composer-row pill", async () => {
  const { api } = fakeApi();
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="provider-shell" sessionId="s1" />);
  await flush();

  const b = bubble(host);
  assert(b, "bubble should render");
  assert(b.classList.contains("ct-chat-model-bubble"), "bubble should expose the composer pill class");
  assert(b.classList.contains("h-7"), "bubble should match composer pill height");
  const wrap = b.parentElement as HTMLDivElement | null;
  assert(wrap, "bubble should have an inline wrapper");
  assert(wrap.classList.contains("inline-flex"), "bubble should fit in the composer pill row");
  assert(wrap.style.right === "", `bubble should not use Tab right edge, got: ${wrap.style.right}`);
  assert(wrap.style.left === "", `bubble should not use Tab left edge, got: ${wrap.style.left}`);

  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("opens a dropdown listing the available models", async () => {
  const { api } = fakeApi();
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="provider-shell" sessionId="s1" />);
  await flush();

  assert(options(host).length === 0, "dropdown should start closed");
  await act(async () => {
    bubble(host)!.click();
  });

  const opts = options(host);
  assert(opts.length === MODELS.length, `dropdown should list ${MODELS.length} models, got ${opts.length}`);
  assert(opts.some((o) => o.textContent?.includes("Model A")), "dropdown should include Model A");

  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("picking a model invokes switch_model and updates the bubble", async () => {
  const { api, switchCalls } = fakeApi();
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="provider-shell" sessionId="s1" />);
  await flush();

  await act(async () => {
    bubble(host)!.click();
  });
  const nextModel = options(host).find((o) => o.getAttribute("data-model-id") === "model-b");
  assert(nextModel, "Model B option should render");
  await act(async () => {
    nextModel.click();
  });
  await flush();

  assert(switchCalls.length === 1, `switch_model should be called once, got ${switchCalls.length}`);
  assert(
    switchCalls[0].pluginId === "provider-shell"
      && switchCalls[0].sessionId === "s1"
      && switchCalls[0].model === "model-b",
    `switch_model called with wrong args: ${JSON.stringify(switchCalls[0])}`,
  );
  assert(bubble(host)?.textContent?.includes("Model B"), "bubble should reflect the newly selected model");
  assert(options(host).length === 0, "dropdown should close after picking");

  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("picking a model that needsConfirm shows the plugin confirm and only switches on OK", async () => {
  const describeCalls: string[] = [];
  const { api, switchCalls } = fakeApi({
    describeModelSwitch: async (_p, _s, model) => {
      describeCalls.push(model);
      return { needsConfirm: true, message: "Loading Model B will unload Model A (VRAM-tight)." };
    },
  });
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="provider-shell" sessionId="s1" />);
  await flush();

  await pick(host, "model-b");

  assert(describeCalls.length === 1 && describeCalls[0] === "model-b", `describeModelSwitch should be asked first, got ${JSON.stringify(describeCalls)}`);
  const switchesBeforeOk = switchCalls.length;
  assert(switchesBeforeOk === 0, "switch must NOT fire before the user confirms");
  const dialog = confirmDialog();
  assert(dialog, "confirm dialog should appear when needsConfirm");
  assert(
    dialog.textContent?.includes("Loading Model B will unload Model A"),
    `dialog should show the PLUGIN message, got: ${dialog.textContent}`,
  );

  const ok = document.querySelector('[data-testid="chat-model-switch-confirm-confirm"]') as HTMLButtonElement | null;
  assert(ok, "confirm OK button should render");
  await act(async () => {
    ok.click();
  });
  await flush();

  assert(switchCalls.length === 1 && switchCalls[0].model === "model-b", `switch should fire once on OK, got ${JSON.stringify(switchCalls)}`);
  assert(!confirmDialog(), "confirm dialog should close after OK");
  assert(bubble(host)?.textContent?.includes("Model B"), "bubble should reflect the switched model");

  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("cancelling the confirm leaves the model unchanged", async () => {
  const { api, switchCalls } = fakeApi({
    describeModelSwitch: async () => ({ needsConfirm: true, message: "disruptive" }),
  });
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="provider-shell" sessionId="s1" />);
  await flush();

  await pick(host, "model-b");
  const cancel = document.querySelector('[data-testid="chat-model-switch-confirm-cancel"]') as HTMLButtonElement | null;
  assert(cancel, "confirm cancel button should render");
  await act(async () => {
    cancel.click();
  });
  await flush();

  assert(switchCalls.length === 0, "switch must NOT fire on cancel");
  assert(!confirmDialog(), "confirm dialog should close after cancel");
  assert(bubble(host)?.textContent?.includes("Model A"), "bubble should keep the original model");

  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("needsConfirm=false switches directly with no confirm dialog", async () => {
  const { api, switchCalls } = fakeApi({
    describeModelSwitch: async () => ({ needsConfirm: false, message: "" }),
  });
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="provider-shell" sessionId="s1" />);
  await flush();

  await pick(host, "model-b");

  assert(!confirmDialog(), "no confirm dialog when needsConfirm=false");
  assert(switchCalls.length === 1 && switchCalls[0].model === "model-b", `switch should fire directly, got ${JSON.stringify(switchCalls)}`);
  assert(bubble(host)?.textContent?.includes("Model B"), "bubble should reflect the switched model");

  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("a plugin without describeModelSwitch falls back to a direct switch (no crash)", async () => {
  // No describeModelSwitch on the api at all — the optional hook is simply absent.
  const { api, switchCalls } = fakeApi();
  assert(api.describeModelSwitch === undefined, "this case models the hook being absent");
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="provider-shell" sessionId="s1" />);
  await flush();

  await pick(host, "model-b");

  assert(!confirmDialog(), "no confirm dialog without the hook");
  assert(switchCalls.length === 1 && switchCalls[0].model === "model-b", `switch should fire directly, got ${JSON.stringify(switchCalls)}`);

  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("updates selector entries exactly when the live plugin catalog changes", async () => {
  const { api } = fakeApi();
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="provider-shell" sessionId="s1" />);
  await flush();

  await act(async () => bubble(host)?.click());
  assert(options(host).map((option) => option.dataset.modelId).join(",") === "model-a,model-b", "initial entries equal catalog");

  catalogModels = [{ id: "model-next", display_name: "Model Next" }];
  await act(async () => modelsUpdated?.());
  await flush();
  assert(options(host).map((option) => option.dataset.modelId).join(",") === "model-next", "mounted selector follows catalog update");

  catalogModels = MODELS;
  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("shows an explicit empty state when the catalog publishes no models", async () => {
  catalogModels = [];
  const { api } = fakeApi();
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="provider-shell" sessionId="s1" />);
  await flush();

  const status = host.querySelector('[data-testid="chat-model-bubble-status"]');
  assert(status?.textContent?.includes("No models published"), "empty catalog is explicit");

  catalogModels = MODELS;
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
  __setInvokeForTest(null);
  __setListenForTest(null);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
