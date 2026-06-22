// Run: npx tsx src/components/Extensions/ChatPreprocessorPresetManager.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { ChatPreprocessorPresetManager } from "./ChatPreprocessorPresetManager";
import {
  __setPreprocessorPresetApiForTest,
  type PreprocessorPresetApi,
  type PreprocessorPreset,
  type PreprocessorPresetState,
} from "./chatPreprocessorPresetApi";

type TestFn = () => void | Promise<void>;

const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT =
    true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", {
    value: window,
    configurable: true,
  });
  Object.defineProperty(globalThis, "document", {
    value: window.document,
    configurable: true,
  });
  Object.defineProperty(globalThis, "navigator", {
    value: window.navigator,
    configurable: true,
  });
  Object.defineProperty(globalThis, "HTMLElement", {
    value: window.HTMLElement,
    configurable: true,
  });
  Object.defineProperty(globalThis, "HTMLButtonElement", {
    value: window.HTMLButtonElement,
    configurable: true,
  });
  Object.defineProperty(globalThis, "HTMLInputElement", {
    value: window.HTMLInputElement,
    configurable: true,
  });
  Object.defineProperty(globalThis, "HTMLTextAreaElement", {
    value: window.HTMLTextAreaElement,
    configurable: true,
  });
  Object.defineProperty(globalThis, "SVGElement", {
    value: window.SVGElement,
    configurable: true,
  });
  Object.defineProperty(globalThis, "Element", {
    value: window.Element,
    configurable: true,
  });
  Object.defineProperty(globalThis, "Node", {
    value: window.Node,
    configurable: true,
  });
  Object.defineProperty(globalThis, "Event", {
    value: window.Event,
    configurable: true,
  });
  Object.defineProperty(globalThis, "CustomEvent", {
    value: window.CustomEvent,
    configurable: true,
  });
  Object.defineProperty(globalThis, "setTimeout", {
    value: window.setTimeout.bind(window),
    configurable: true,
  });
}

async function render(
  ui: React.ReactElement,
): Promise<{ host: HTMLDivElement; root: Root }> {
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

function fakeApi(
  initial: PreprocessorPresetState,
  overrides: Partial<PreprocessorPresetApi> = {},
): {
  api: PreprocessorPresetApi;
  saves: PreprocessorPresetState[];
} {
  const saves: PreprocessorPresetState[] = [];
  const api: PreprocessorPresetApi = {
    load: async () => structuredClone(initial),
    save: async (_pluginId, state) => {
      saves.push(
        structuredClone({
          ...state,
          presets: state.presets.filter((p) => !p.system),
        }),
      );
    },
    ...overrides,
  };
  return { api, saves };
}

function presetRows(host: HTMLElement): HTMLElement[] {
  return Array.from(host.querySelectorAll('[data-testid="ppre-row"]'));
}
function byTestId(host: HTMLElement, id: string): HTMLElement | null {
  return host.querySelector(`[data-testid="${id}"]`);
}
async function setInputValue(
  el: HTMLInputElement | HTMLTextAreaElement,
  value: string,
) {
  const proto =
    el instanceof HTMLTextAreaElement
      ? window.HTMLTextAreaElement.prototype
      : window.HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  await act(async () => {
    setter?.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });
}

const SEED: PreprocessorPresetState = {
  presets: [
    {
      id: "p1",
      name: "Formal",
      description: "Makes messages more formal",
      config: { tone: "formal" },
    },
    {
      id: "p2",
      name: "Concise",
      description: "Trims verbose messages",
    },
  ],
  activePreset: "p1",
};

setupDom();

test("renders the preset list from load()", async () => {
  const { api } = fakeApi(SEED);
  __setPreprocessorPresetApiForTest(api);
  const { host, root } = await render(
    <ChatPreprocessorPresetManager pluginId="my-preprocessor" />,
  );
  await flush();

  const rows = presetRows(host);
  // SEED has 2 user presets + 1 system preset (Memory recall) always injected = 3.
  assert(rows.length === 3, `expected 3 preset rows, got ${rows.length}`);
  assert(
    host.textContent?.includes("Memory recall"),
    "list should show the system Memory recall preset",
  );
  assert(host.textContent?.includes("Formal"), "list should show 'Formal'");
  assert(host.textContent?.includes("Concise"), "list should show 'Concise'");

  __setPreprocessorPresetApiForTest(null);
  await act(async () => root.unmount());
});

test("marks the active preset", async () => {
  const { api } = fakeApi(SEED);
  __setPreprocessorPresetApiForTest(api);
  const { host, root } = await render(
    <ChatPreprocessorPresetManager pluginId="my-preprocessor" />,
  );
  await flush();

  const active = byTestId(host, "ppre-row-active");
  assert(active, "an active preset row should be marked");
  assert(
    active.getAttribute("data-preset-id") === "p1",
    `active should be p1, got ${active.getAttribute("data-preset-id")}`,
  );

  __setPreprocessorPresetApiForTest(null);
  await act(async () => root.unmount());
});

test("creating a preset saves the appended list", async () => {
  const { api, saves } = fakeApi(SEED);
  __setPreprocessorPresetApiForTest(api);
  const { host, root } = await render(
    <ChatPreprocessorPresetManager pluginId="my-preprocessor" />,
  );
  await flush();

  await act(async () => {
    byTestId(host, "ppre-new")!.click();
  });
  await flush();

  assert(saves.length === 1, `create should save once, got ${saves.length}`);
  const last = saves[saves.length - 1];
  // System presets are filtered out before saving; SEED has 2 user presets + 1 new.
  const userPresets = last.presets.filter((p: PreprocessorPreset) => !p.system);
  assert(
    userPresets.length === 3,
    `should have 3 user presets after create, got ${userPresets.length}`,
  );
  const created = userPresets[userPresets.length - 1];
  assert(
    created.name.length > 0 && created.id.length > 0,
    "created preset should have a name and id",
  );

  __setPreprocessorPresetApiForTest(null);
  await act(async () => root.unmount());
});

test("editing the name saves the updated preset", async () => {
  const { api, saves } = fakeApi(SEED);
  __setPreprocessorPresetApiForTest(api);
  const { host, root } = await render(
    <ChatPreprocessorPresetManager pluginId="my-preprocessor" />,
  );
  await flush();

  const formalRow = presetRows(host).find(
    (r) => r.getAttribute("data-preset-id") === "p1",
  );
  assert(formalRow, "Formal (p1) row should exist");
  await act(async () => {
    formalRow!.click();
  });
  const nameInput = byTestId(host, "ppre-name") as HTMLInputElement;
  assert(nameInput, "name input should render for the selected preset");
  await setInputValue(nameInput, "Very Formal");
  await flush();

  assert(saves.length >= 1, "editing should persist");
  const last = saves[saves.length - 1];
  const edited = last.presets.find((p: PreprocessorPreset) => p.id === "p1");
  assert(
    edited?.name === "Very Formal",
    `name should persist as 'Very Formal', got ${edited?.name}`,
  );

  __setPreprocessorPresetApiForTest(null);
  await act(async () => root.unmount());
});

test("editing the description saves it", async () => {
  const { api, saves } = fakeApi(SEED);
  __setPreprocessorPresetApiForTest(api);
  const { host, root } = await render(
    <ChatPreprocessorPresetManager pluginId="my-preprocessor" />,
  );
  await flush();

  const conciseRow = presetRows(host).find(
    (r) => r.getAttribute("data-preset-id") === "p2",
  );
  assert(conciseRow, "Concise (p2) row should exist");
  await act(async () => {
    conciseRow!.click();
  });
  const descInput = byTestId(host, "ppre-description") as HTMLInputElement;
  assert(descInput, "description input should render");
  await setInputValue(descInput, "Updated description");
  await flush();

  const last = saves[saves.length - 1];
  const edited = last.presets.find((p: PreprocessorPreset) => p.id === "p2");
  assert(
    edited?.description === "Updated description",
    `description should persist, got ${edited?.description}`,
  );

  __setPreprocessorPresetApiForTest(null);
  await act(async () => root.unmount());
});

test("valid JSON config is parsed and saved", async () => {
  const { api, saves } = fakeApi(SEED);
  __setPreprocessorPresetApiForTest(api);
  const { host, root } = await render(
    <ChatPreprocessorPresetManager pluginId="my-preprocessor" />,
  );
  await flush();

  const conciseRow = presetRows(host).find(
    (r) => r.getAttribute("data-preset-id") === "p2",
  );
  assert(conciseRow, "Concise (p2) row should exist");
  await act(async () => {
    conciseRow!.click();
  });
  const configArea = byTestId(host, "ppre-params") as HTMLTextAreaElement;
  assert(configArea, "config textarea should render");
  await setInputValue(configArea, '{"maxWords": 50}');
  await flush();

  const last = saves[saves.length - 1];
  const edited = last.presets.find((p: PreprocessorPreset) => p.id === "p2");
  assert(
    (edited?.config as { maxWords?: number } | undefined)?.maxWords === 50,
    `config.maxWords should be 50, got ${JSON.stringify(edited?.config)}`,
  );

  __setPreprocessorPresetApiForTest(null);
  await act(async () => root.unmount());
});

test("invalid JSON config is not saved (no state mutation)", async () => {
  const { api, saves } = fakeApi(SEED);
  __setPreprocessorPresetApiForTest(api);
  const { host, root } = await render(
    <ChatPreprocessorPresetManager pluginId="my-preprocessor" />,
  );
  await flush();

  const initialSaves = saves.length;
  const formalRow = presetRows(host).find(
    (r) => r.getAttribute("data-preset-id") === "p1",
  );
  assert(formalRow, "Formal (p1) row should exist");
  await act(async () => {
    formalRow!.click();
  });
  const configArea = byTestId(host, "ppre-params") as HTMLTextAreaElement;
  await setInputValue(configArea, '{"broken": }');
  await flush();

  assert(
    saves.length === initialSaves,
    `invalid JSON should not trigger a save, got ${saves.length - initialSaves} extra saves`,
  );

  __setPreprocessorPresetApiForTest(null);
  await act(async () => root.unmount());
});

test("setting a preset active persists activePreset and reflects in UI", async () => {
  const { api, saves } = fakeApi(SEED);
  __setPreprocessorPresetApiForTest(api);
  const { host, root } = await render(
    <ChatPreprocessorPresetManager pluginId="my-preprocessor" />,
  );
  await flush();

  const conciseRow = presetRows(host).find(
    (r) => r.getAttribute("data-preset-id") === "p2",
  );
  assert(conciseRow, "Concise (p2) row should exist");
  await act(async () => {
    conciseRow!.click();
  });
  await act(async () => {
    byTestId(host, "ppre-set-active")!.click();
  });
  await flush();

  const last = saves[saves.length - 1];
  assert(
    last.activePreset === "p2",
    `activePreset should be p2, got ${last.activePreset}`,
  );
  const active = byTestId(host, "ppre-row-active");
  assert(
    active?.getAttribute("data-preset-id") === "p2",
    "UI should reflect p2 active",
  );

  __setPreprocessorPresetApiForTest(null);
  await act(async () => root.unmount());
});

test("duplicate creates a copy with '(copy)' suffix and saves", async () => {
  const { api, saves } = fakeApi(SEED);
  __setPreprocessorPresetApiForTest(api);
  const { host, root } = await render(
    <ChatPreprocessorPresetManager pluginId="my-preprocessor" />,
  );
  await flush();

  // System preset is first; click the first user preset row (Formal = p1).
  const rows = presetRows(host);
  const formalRow = rows.find(
    (r) => r.getAttribute("data-preset-id") === "p1",
  );
  assert(formalRow, "Formal (p1) row should exist");
  await act(async () => {
    formalRow!.click();
  });
  await act(async () => {
    byTestId(host, "ppre-duplicate")!.click();
  });
  await flush();

  assert(saves.length === 1, `duplicate should save once, got ${saves.length}`);
  const last = saves[saves.length - 1];
  const userPresets = last.presets.filter((p: PreprocessorPreset) => !p.system);
  assert(
    userPresets.length === 3,
    `should have 3 user presets after duplicate, got ${userPresets.length}`,
  );
  const copy = userPresets[userPresets.length - 1];
  assert(
    copy.name.includes("copy"),
    `copy name should include 'copy', got ${copy.name}`,
  );
  assert(copy.id !== "p1", "copy should have a new id");

  __setPreprocessorPresetApiForTest(null);
  await act(async () => root.unmount());
});

test("deleting a preset saves the list without it", async () => {
  const { api, saves } = fakeApi(SEED);
  __setPreprocessorPresetApiForTest(api);
  const { host, root } = await render(
    <ChatPreprocessorPresetManager pluginId="my-preprocessor" />,
  );
  await flush();

  const conciseRow = presetRows(host).find(
    (r) => r.getAttribute("data-preset-id") === "p2",
  );
  assert(conciseRow, "Concise (p2) row should exist");
  await act(async () => {
    conciseRow!.click();
  });
  await act(async () => {
    byTestId(host, "ppre-delete")!.click();
  });
  await flush();

  const last = saves[saves.length - 1];
  const userPresets = last.presets.filter((p: PreprocessorPreset) => !p.system);
  assert(
    userPresets.length === 1,
    `should have 1 user preset after delete, got ${userPresets.length}`,
  );
  assert(
    !last.presets.some((p: PreprocessorPreset) => p.id === "p2"),
    "p2 should be gone",
  );

  __setPreprocessorPresetApiForTest(null);
  await act(async () => root.unmount());
});

test("shows empty user-preset hint when only system presets exist", async () => {
  const { api } = fakeApi({ presets: [], activePreset: null });
  __setPreprocessorPresetApiForTest(api);
  const { host, root } = await render(
    <ChatPreprocessorPresetManager pluginId="my-preprocessor" />,
  );
  await flush();

  // System presets are always injected, so rows > 0; user-preset empty hint still shows.
  assert(byTestId(host, "ppre-empty"), "empty-state hint should render when no user presets");
  assert(byTestId(host, "ppre-new"), "New preset button stays available");

  __setPreprocessorPresetApiForTest(null);
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
  console.log(
    `ChatPreprocessorPresetManager tests: ${tests.length - failed}/${tests.length} passed in ${elapsedMs.toFixed(1)}ms`,
  );
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
