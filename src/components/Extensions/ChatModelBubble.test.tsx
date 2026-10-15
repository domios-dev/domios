import { test, beforeAll, beforeEach } from "vitest";
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
import { invalidateAgentProviders } from "../../hooks/useAgentProviders";

beforeEach(() => invalidateAgentProviders());

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
let catalogueVersion = 1;
let modelsUpdated: (() => void) | undefined;

function catalogProvider(): ProviderDescriptor {
  return {
    schema_version: 1,
    id: "provider-shell",
    catalogue_version: catalogueVersion,
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

function buttonNamed(host: HTMLElement, name: string): HTMLButtonElement | null {
  return Array.from(host.querySelectorAll<HTMLButtonElement>("button"))
    .find((button) => button.getAttribute("aria-label") === name || button.textContent?.trim() === name) ?? null;
}

function modelOptionNames(host: HTMLElement): string[] {
  return Array.from(host.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"][aria-label]'))
    .map((button) => button.getAttribute("aria-label")!);
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

beforeAll(setupDom);
__setInvokeForTest(async <T,>(command: string): Promise<T> => {
  if (command === "list_agent_providers") return [catalogProvider()] as unknown as T;
  throw new Error(`Unexpected command: ${command}`);
});
__setListenForTest(async <T,>(event: string, handler: (event: { payload: T }) => void) => {
  if (event === "model-catalogue-changed") {
    modelsUpdated = () => handler({ payload: { provider: "provider-shell", version: catalogueVersion } as T });
  }
  return () => {
    if (event === "model-catalogue-changed") modelsUpdated = undefined;
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

test("renders the session model instead of the provider default", async () => {
  const { api } = fakeApi({ sessionInfo: async () => ({ model: "model-b" }) });
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="provider-shell" sessionId="s1" />);
  await flush();

  const b = bubble(host);
  assert(b, "bubble should render");
  assert(catalogProvider().default_model === "model-a", "fixture provider default should differ");
  assert(b.textContent?.includes("Model B"), `bubble should show the session model, got: ${b.textContent}`);
  assert(!b.textContent?.includes("Model A"), "bubble must not fall back to the provider default");

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
  assert(confirmDialog() === null, "unsupported confirmation control stays absent");

  __setChatModelBubbleApiForTest(null);
  await act(async () => root.unmount());
});

test("updates selector entries exactly when the live plugin catalog changes", async () => {
  const { api } = fakeApi();
  __setChatModelBubbleApiForTest(api);
  const { host, root } = await render(<ChatModelBubble pluginId="provider-shell" sessionId="s1" />);
  await flush();

  await act(async () => buttonNamed(host, "Model A")?.click());
  assert(modelOptionNames(host).join(",") === "Model A,Model B", "initial accessible model choices equal catalog");

  catalogModels = [{ id: "model-next", display_name: "Model Next" }];
  catalogueVersion = 2;
  await act(async () => modelsUpdated?.());
  await flush();
  assert(modelOptionNames(host).join(",") === "Model Next", "mounted selector follows the versioned catalogue event");

  catalogModels = MODELS;
  catalogueVersion = 1;
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



test("the default API switches directly through the mapped daemon command", async () => {
  __setChatModelBubbleApiForTest(null);
  const calls: string[] = [];
  __setInvokeForTest(async <T,>(command: string) => {
    calls.push(command);
    if (command === "list_agent_providers") return [catalogProvider()] as T;
    if (command === "chatbackend_session_info") return { model: "model-a" } as T;
    if (command === "chatbackend_switch_model") return { model: "model-b" } as T;
    throw new Error(`Unexpected command: ${command}`);
  });
  const { host, root } = await render(<ChatModelBubble pluginId="provider-shell" sessionId="s1" />);
  await pick(host, "model-b");
  assert(calls.filter(c => c.startsWith("chatbackend_")).join(",") === "chatbackend_session_info,chatbackend_switch_model", "only supported session and switch commands run");
  assert(bubble(host)?.textContent?.includes("Model B"), "selected model is rendered");
  assert(confirmDialog() === null, "unsupported confirmation control stays absent");
  await act(async () => root.unmount());
});
