// Run: npx tsx src/components/Settings/SchemaRenderer.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { SchemaRenderer } from "./SchemaRenderer";
import type { AppConfig, ProviderConfig, SettingField } from "../../lib/types";

type TestFn = () => void | Promise<void>;

const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setNativeValue(element: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string) {
  const prototype = Object.getPrototypeOf(element) as HTMLElement;
  const descriptor = Object.getOwnPropertyDescriptor(prototype, "value");
  descriptor?.set?.call(element, value);
}

function dispatchFormChange(element: HTMLElement) {
  element.dispatchEvent(new window.Event("input", { bubbles: true }));
  element.dispatchEvent(new window.Event("change", { bubbles: true }));
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLInputElement", { value: window.HTMLInputElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLTextAreaElement", { value: window.HTMLTextAreaElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLSelectElement", { value: window.HTMLSelectElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
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

const config = {
  agents: {},
} as AppConfig;

const schema: SettingField[] = [
  {
    kind: "array",
    key: "presets",
    label: "Presets",
    item_label: "Preset",
    item: {
      kind: "object",
      fields: [
        { key: "id", label: "ID", kind: "text" },
        { key: "name", label: "Name", kind: "text" },
        { key: "systemPrompt", label: "System Prompt", kind: "textarea" },
        { key: "model", label: "Model", kind: "text" },
        { key: "params", label: "Params", kind: "json" },
      ],
    },
  },
  {
    kind: "select",
    key: "defaultPreset",
    label: "Default Preset",
    options: [
      ["codeterm", "CodeTerm Assistant"],
      ["rhymes", "Rhyme Bot"],
    ],
  },
];

setupDom();

test("schema toggles use the canonical liquid switch", async () => {
  const patches: Partial<ProviderConfig>[] = [];
  const { host, root } = await render(
    <SchemaRenderer
      schema={[{
        kind: "toggle",
        key: "enabled",
        label: "Enable feature",
        default: false,
      }]}
      providerId="test-provider"
      config={config}
      values={{ enabled: false }}
      onChange={(_providerId, patch) => patches.push(patch)}
    />,
  );

  const toggle = host.querySelector('[data-testid="settings-schema-toggle"]');
  assert(toggle instanceof HTMLButtonElement, "schema toggle should render as a button");
  assert(toggle.classList.contains("ct-liquid-toggle"), "schema toggle should use LiquidToggle");
  assert(toggle.getAttribute("role") === "switch", "schema toggle should retain switch semantics");
  assert(toggle.getAttribute("aria-checked") === "false", "schema toggle should expose its state");

  await act(async () => toggle.click());
  assert(patches.length === 1 && patches[0].enabled === true, "schema toggle should emit the next value");
  assert(host.querySelector(".ct-liquid-toggle__knob--travel"), "schema toggle should start liquid travel");

  await act(async () => root.unmount());
});

test("presets array-of-objects renders and serializes expected config shape", async () => {
  let values: ProviderConfig = {
    defaultPreset: "codeterm",
    presets: [
      {
        id: "codeterm",
        name: "CodeTerm Assistant",
        systemPrompt: "You are CodeTerm.",
        model: "",
        params: { temperature: 0.7 },
      },
    ],
  };
  const patches: Partial<ProviderConfig>[] = [];

  function onChange(_providerId: string, patch: Partial<ProviderConfig>) {
    patches.push(patch);
    values = { ...values, ...patch };
  }

  const { host, root } = await render(
    <SchemaRenderer
      schema={schema}
      providerId="lmstudio"
      config={config}
      values={values}
      onChange={onChange}
    />,
  );

  const rerender = async () => {
    await act(async () => {
      root.render(
        <SchemaRenderer
          schema={schema}
          providerId="lmstudio"
          config={config}
          values={values}
          onChange={onChange}
        />,
      );
    });
  };

  assert(host.textContent?.includes("CodeTerm Assistant"), "existing preset row should render");

  const nameInput = host.querySelector('[data-testid="settings-schema-array-input"][data-settings-field="presets.0.name"]');
  assert(nameInput instanceof HTMLInputElement, "preset name input should render");
  await act(async () => {
    setNativeValue(nameInput, "CodeTerm Shell");
    dispatchFormChange(nameInput);
  });
  const expectedNamePatch = {
    presets: [
      {
        id: "codeterm",
        name: "CodeTerm Shell",
        systemPrompt: "You are CodeTerm.",
        model: "",
        params: { temperature: 0.7 },
      },
    ],
  };
  assert(
    JSON.stringify(patches[patches.length - 1]) === JSON.stringify(expectedNamePatch),
    `editing a preset field should serialize the full presets array; got ${JSON.stringify(patches[patches.length - 1])}`,
  );
  await rerender();

  const promptInput = host.querySelector('[data-testid="settings-schema-array-textarea"][data-settings-field="presets.0.systemPrompt"]');
  assert(promptInput instanceof HTMLTextAreaElement, "systemPrompt textarea should render");
  await act(async () => {
    setNativeValue(promptInput, "You answer only in rhymes.");
    dispatchFormChange(promptInput);
  });
  await rerender();

  const paramsInput = host.querySelector('[data-testid="settings-schema-array-json"][data-settings-field="presets.0.params"]');
  assert(paramsInput instanceof HTMLTextAreaElement, "params JSON textarea should render");
  await act(async () => {
    setNativeValue(paramsInput, '{ "temperature": 0.2, "top_p": 0.9 }');
    dispatchFormChange(paramsInput);
  });
  assert(JSON.stringify(patches[patches.length - 1]) === JSON.stringify({
    presets: [
      {
        id: "codeterm",
        name: "CodeTerm Shell",
        systemPrompt: "You answer only in rhymes.",
        model: "",
        params: { temperature: 0.2, top_p: 0.9 },
      },
    ],
  }), "params should serialize as an object, not a JSON string");
  await rerender();

  const defaultSelect = host.querySelector('[data-testid="settings-schema-select"][data-settings-field="defaultPreset"]');
  assert(defaultSelect instanceof HTMLSelectElement, "defaultPreset select should render");
  await act(async () => {
    setNativeValue(defaultSelect, "rhymes");
    dispatchFormChange(defaultSelect);
  });
  assert(JSON.stringify(patches[patches.length - 1]) === JSON.stringify({ defaultPreset: "rhymes" }), "defaultPreset should serialize as a sibling setting");
  await rerender();

  const addButton = host.querySelector('[data-testid="settings-schema-array-add"][data-settings-field="presets"]');
  assert(addButton instanceof HTMLButtonElement, "add preset button should render");
  await act(async () => {
    addButton.click();
  });
  assert((values.presets as unknown[]).length === 2, "add should append a preset object");
  await rerender();

  const removeButton = host.querySelector('[data-testid="settings-schema-array-remove"][data-settings-field="presets.1"]');
  assert(removeButton instanceof HTMLButtonElement, "remove preset button should render");
  await act(async () => {
    removeButton.click();
  });
  assert(JSON.stringify(patches[patches.length - 1]) === JSON.stringify({
    presets: [
      {
        id: "codeterm",
        name: "CodeTerm Shell",
        systemPrompt: "You answer only in rhymes.",
        model: "",
        params: { temperature: 0.2, top_p: 0.9 },
      },
    ],
  }), "remove should serialize the remaining presets array");

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
  console.log(`SchemaRenderer tests: ${tests.length - failed}/${tests.length} passed in ${elapsedMs.toFixed(1)}ms`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
