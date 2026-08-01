import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import type { ProviderDescriptor } from "../../lib/types";
import { defaultConfig } from "../../hooks/useConfig";
import { MemorySettingsTab } from "./MemorySettingsTab";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
}

function provider(): ProviderDescriptor {
  return {
    schema_version: 1,
    id: "provider-a",
    display_name: "Provider A",
    transport: "pty",
    system_prompt_delivery: "external",
    available_models: [
      { id: "model-a", display_name: "Model A" },
      { id: "model-b", display_name: "Model B" },
    ],
    default_model: "model-a",
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
  };
}

async function main() {
  setupDom();

  let resolveCatalog!: (providers: ProviderDescriptor[]) => void;
  const catalog = new Promise<ProviderDescriptor[]>((resolve) => {
    resolveCatalog = resolve;
  });
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "list_agent_providers") return await catalog as unknown as T;
    throw new Error(`Unexpected command: ${command}`);
  });
  __setListenForTest(async () => () => {});

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const config = defaultConfig();
  config.api = undefined as never;

  await act(async () => {
    root.render(<MemorySettingsTab config={config} onUpdate={() => {}} />);
  });

  const extraction = host.querySelector('[data-testid="memory-settings-extraction-model"]') as HTMLSelectElement | null;
  assert(extraction, "extraction model selector renders before catalog arrival");
  assert(extraction.disabled, "selector is disabled while catalog loads");
  assert(extraction.textContent?.includes("Loading model catalog"), "loading state is explicit");
  assert(host.querySelector('input[data-testid="memory-settings-extraction-model"]') === null, "no free-text fallback renders");

  await act(async () => {
    resolveCatalog([provider()]);
    await catalog;
    await Promise.resolve();
  });

  const optionValues = Array.from(extraction.options).map((option) => option.value);
  assert(optionValues.join(",") === ",model-a,model-b", `selector entries must equal the catalog, got ${optionValues}`);
  assert(!extraction.disabled, "selector enables when catalog models arrive");

  await act(async () => root.unmount());
  __setInvokeForTest(null);
  __setListenForTest(null);

  console.log("✓ Memory model selector waits for and renders the plugin catalog exactly");
}

void main();
