// Run: npx tsx src/components/Extensions/PresetManager.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { PresetManager } from "./PresetManager";
import {
  __setPresetManagerApiForTest,
  type PresetManagerApi,
  type Preset,
  type PresetState,
} from "./presetManagerApi";
import { PluginDetailPanel } from "./PluginDetailPanel";
import type { CatalogEntry } from "./pluginCatalog";
import { useAgentProviderCachePublisher } from "../../hooks/useAgentProviders";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import type { ModelInfo, ProviderDescriptor } from "../../lib/types";

type TestFn = () => void | Promise<void>;

const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLInputElement", { value: window.HTMLInputElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLTextAreaElement", { value: window.HTMLTextAreaElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLSelectElement", { value: window.HTMLSelectElement, configurable: true });
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

function catalogProvider(models: ModelInfo[]): ProviderDescriptor {
  return {
    schema_version: 1,
    id: "catalog-provider",
    display_name: "Catalog Provider",
    transport: "chat_backend",
    behavior_config: { model: "presets_and_system_prompt" },
    system_prompt_delivery: "external",
    available_models: models,
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
  };
}

const CATALOG_ENTRY: CatalogEntry = {
  id: "catalog-provider",
  displayName: "Catalog Provider",
  installed: true,
  inMarketplace: false,
  system: false,
  verified: true,
  removable: false,
  dev: true,
  compatible: true,
  updateAvailable: false,
};

function CatalogPluginDetail() {
  useAgentProviderCachePublisher();
  return (
    <PluginDetailPanel
      entry={CATALOG_ENTRY}
      config={null}
      busy={null}
      onAction={() => {}}
      onSave={async () => {}}
      onUpdateFullConfig={() => {}}
      onGone={() => {}}
      activityDisplayMap={new Map()}
    />
  );
}

function fakeApi(
  initial: PresetState,
  overrides: Partial<PresetManagerApi> = {},
): {
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
    ...overrides,
  };
  return { api, saves, applies };
}

function presetRows(host: HTMLElement): HTMLElement[] {
  return Array.from(host.querySelectorAll('[data-testid="preset-row"]'));
}
function byTestId(host: HTMLElement, id: string): HTMLElement | null {
  return host.querySelector(`[data-testid="${id}"]`);
}
function presetModelIds(host: HTMLElement): string[] {
  const select = byTestId(host, "preset-model") as HTMLSelectElement | null;
  if (!select) return [];
  return Array.from(select.options).map((option) => option.value).filter(Boolean);
}
async function setInputValue(el: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string) {
  const proto =
    el instanceof HTMLTextAreaElement
      ? window.HTMLTextAreaElement.prototype
      : el instanceof HTMLSelectElement
        ? window.HTMLSelectElement.prototype
        : window.HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  await act(async () => {
    setter?.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });
}

const SEED: PresetState = {
  presets: [
    { id: "p1", name: "Coder", systemPrompt: "You write code.", model: "qwen-2.5", params: { temperature: 0.2 } },
    { id: "p2", name: "Writer", systemPrompt: "You write prose." },
  ],
  activePreset: "p1",
};

setupDom();

test("renders the preset list from load()", async () => {
  const { api } = fakeApi(SEED);
  __setPresetManagerApiForTest(api);
  const { host, root } = await render(<PresetManager pluginId="lmstudio" models={MODELS} />);
  await flush();

  const rows = presetRows(host);
  assert(rows.length === 2, `expected 2 preset rows, got ${rows.length}`);
  assert(host.textContent?.includes("Coder"), "list should show 'Coder'");
  assert(host.textContent?.includes("Writer"), "list should show 'Writer'");

  __setPresetManagerApiForTest(null);
  await act(async () => root.unmount());
});

test("marks the active preset", async () => {
  const { api } = fakeApi(SEED);
  __setPresetManagerApiForTest(api);
  const { host, root } = await render(<PresetManager pluginId="lmstudio" models={MODELS} />);
  await flush();

  const active = byTestId(host, "preset-row-active");
  assert(active, "an active preset row should be marked");
  assert(active.getAttribute("data-preset-id") === "p1", `active should be p1, got ${active.getAttribute("data-preset-id")}`);

  __setPresetManagerApiForTest(null);
  await act(async () => root.unmount());
});

test("creating a preset saves the appended list", async () => {
  const { api, saves } = fakeApi(SEED);
  __setPresetManagerApiForTest(api);
  const { host, root } = await render(<PresetManager pluginId="lmstudio" models={MODELS} />);
  await flush();

  await act(async () => {
    byTestId(host, "preset-new")!.click();
  });
  await flush();

  assert(saves.length === 1, `create should save once, got ${saves.length}`);
  const last = saves[saves.length - 1];
  assert(last.presets.length === 3, `should have 3 presets after create, got ${last.presets.length}`);
  const created = last.presets[last.presets.length - 1];
  assert(created.name.length > 0 && created.id.length > 0, "created preset should have a name and id");

  __setPresetManagerApiForTest(null);
  await act(async () => root.unmount());
});

test("editing the name saves the updated preset", async () => {
  const { api, saves } = fakeApi(SEED);
  __setPresetManagerApiForTest(api);
  const { host, root } = await render(<PresetManager pluginId="lmstudio" models={MODELS} />);
  await flush();

  // Select the first preset, then edit its name.
  await act(async () => {
    presetRows(host)[0].click();
  });
  const nameInput = byTestId(host, "preset-name") as HTMLInputElement;
  assert(nameInput, "name input should render for the selected preset");
  await setInputValue(nameInput, "Coder Pro");
  await flush();

  assert(saves.length >= 1, "editing should persist");
  const last = saves[saves.length - 1];
  const edited = last.presets.find((p: Preset) => p.id === "p1");
  assert(edited?.name === "Coder Pro", `name should persist as 'Coder Pro', got ${edited?.name}`);

  __setPresetManagerApiForTest(null);
  await act(async () => root.unmount());
});

test("binding a model to a preset persists preset.model", async () => {
  const { api, saves } = fakeApi(SEED);
  __setPresetManagerApiForTest(api);
  const { host, root } = await render(<PresetManager pluginId="lmstudio" models={MODELS} />);
  await flush();

  await act(async () => {
    presetRows(host)[1].click(); // p2 (Writer) has no model
  });
  const modelSelect = byTestId(host, "preset-model") as HTMLSelectElement;
  assert(modelSelect, "model picker should render");
  await setInputValue(modelSelect, "llama-3");
  await flush();

  const last = saves[saves.length - 1];
  const bound = last.presets.find((p: Preset) => p.id === "p2");
  assert(bound?.model === "llama-3", `model should bind to llama-3, got ${bound?.model}`);

  __setPresetManagerApiForTest(null);
  await act(async () => root.unmount());
});

test("setting a preset active persists activePreset and reflects in UI", async () => {
  const { api, saves } = fakeApi(SEED);
  __setPresetManagerApiForTest(api);
  const { host, root } = await render(<PresetManager pluginId="lmstudio" models={MODELS} />);
  await flush();

  await act(async () => {
    presetRows(host)[1].click(); // select Writer (p2)
  });
  await act(async () => {
    byTestId(host, "preset-set-active")!.click();
  });
  await flush();

  const last = saves[saves.length - 1];
  assert(last.activePreset === "p2", `activePreset should be p2, got ${last.activePreset}`);
  const active = byTestId(host, "preset-row-active");
  assert(active?.getAttribute("data-preset-id") === "p2", "UI should reflect p2 active");

  __setPresetManagerApiForTest(null);
  await act(async () => root.unmount());
});

test("setting active applies the bound model to a live session", async () => {
  const { api, applies } = fakeApi(SEED);
  __setPresetManagerApiForTest(api);
  const { host, root } = await render(
    <PresetManager pluginId="lmstudio" models={MODELS} sessionId="s1" />,
  );
  await flush();

  await act(async () => {
    presetRows(host)[0].click(); // Coder (p1) bound to qwen-2.5
  });
  await act(async () => {
    byTestId(host, "preset-set-active")!.click();
  });
  await flush();

  assert(applies.length === 1, `should apply once to the session, got ${applies.length}`);
  assert(
    applies[0].pluginId === "lmstudio" && applies[0].sessionId === "s1" && applies[0].model === "qwen-2.5",
    `apply called with wrong args: ${JSON.stringify(applies[0])}`,
  );

  __setPresetManagerApiForTest(null);
  await act(async () => root.unmount());
});

test("deleting a preset saves the list without it", async () => {
  const { api, saves } = fakeApi(SEED);
  __setPresetManagerApiForTest(api);
  const { host, root } = await render(<PresetManager pluginId="lmstudio" models={MODELS} />);
  await flush();

  await act(async () => {
    presetRows(host)[1].click(); // Writer (p2)
  });
  await act(async () => {
    byTestId(host, "preset-delete")!.click();
  });
  await flush();

  const last = saves[saves.length - 1];
  assert(last.presets.length === 1, `should have 1 preset after delete, got ${last.presets.length}`);
  assert(!last.presets.some((p: Preset) => p.id === "p2"), "p2 should be gone");

  __setPresetManagerApiForTest(null);
  await act(async () => root.unmount());
});

test("shows an empty state when there are no presets", async () => {
  const { api } = fakeApi({ presets: [], activePreset: null });
  __setPresetManagerApiForTest(api);
  const { host, root } = await render(<PresetManager pluginId="lmstudio" models={MODELS} />);
  await flush();

  assert(presetRows(host).length === 0, "no rows when empty");
  assert(byTestId(host, "preset-empty"), "an empty-state hint should render");
  assert(byTestId(host, "preset-new"), "the New preset action stays available when empty");

  __setPresetManagerApiForTest(null);
  await act(async () => root.unmount());
});

test("catalog refresh hides stale preset models through loading and error before publishing new entries", async () => {
  const catalogA = [{ id: "model-a", display_name: "Model A" }];
  const catalogB = [{ id: "model-b", display_name: "Model B" }];
  const failedRefresh = deferred<ProviderDescriptor[]>();
  let fetches = 0;
  let modelsUpdated: (() => void) | undefined;

  setAgentDescriptors([]);
  __setPresetManagerApiForTest(fakeApi({
    presets: [{ id: "preset", name: "Preset" }],
    activePreset: "preset",
  }).api);
  __setInvokeForTest(<T,>(command: string): Promise<T> => {
    assert(command === "list_agent_providers", `unexpected command: ${command}`);
    fetches += 1;
    if (fetches === 1) return Promise.resolve([catalogProvider(catalogA)]) as unknown as Promise<T>;
    if (fetches === 2) return failedRefresh.promise as unknown as Promise<T>;
    if (fetches === 3) return Promise.resolve([catalogProvider(catalogB)]) as unknown as Promise<T>;
    return Promise.reject(new Error(`unexpected catalog fetch ${fetches}`));
  });
  __setListenForTest(async <T,>(event: string, handler: (event: { payload: T }) => void) => {
    if (event === "models-updated") {
      modelsUpdated = () => handler({ payload: { provider: "catalog-provider" } as T });
    }
    return () => {
      if (event === "models-updated") modelsUpdated = undefined;
    };
  });

  const { host, root } = await render(<CatalogPluginDetail />);
  await flush();
  assert(presetModelIds(host).join(",") === "model-a", "preset selector starts with exact catalog A entries");
  assert(modelsUpdated, "models-updated listener should be active");

  await act(async () => modelsUpdated?.());
  assert(presetModelIds(host).length === 0, "preset selector exposes no catalog A entries while refreshing");

  await act(async () => {
    failedRefresh.reject(new Error("catalog unavailable"));
    await Promise.resolve();
    await Promise.resolve();
  });
  await flush();
  assert(presetModelIds(host).length === 0, "preset selector exposes no retained catalog A entries after error");

  await act(async () => modelsUpdated?.());
  await flush();
  assert(presetModelIds(host).join(",") === "model-b", "explicit event publishes exact catalog B entries");

  __setInvokeForTest(null);
  __setListenForTest(null);
  __setPresetManagerApiForTest(null);
  await act(async () => {
    setAgentDescriptors([]);
    root.unmount();
  });
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
  console.log(`PresetManager tests: ${tests.length - failed}/${tests.length} passed in ${elapsedMs.toFixed(1)}ms`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
