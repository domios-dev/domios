// Run: npx tsx src/components/MemoryPanel/MemorySettingsTab.numeric.test.tsx

import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import type { AppConfig, ProviderDescriptor } from "../../lib/types";
import { defaultConfig } from "../../hooks/useConfig";
import { MemorySettingsTab } from "./MemorySettingsTab";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const win = new Window({ url: "http://localhost/" });
  Object.assign(win, {
    __CODETERM_DAEMON__: {
      origin: /^https?:/.test(win.location.origin) ? win.location.origin : "http://127.0.0.1:7686",
      token: "test-token",
    },
  });
  for (const key of [
    "window",
    "document",
    "HTMLElement",
    "HTMLInputElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
    "KeyboardEvent",
    "FocusEvent",
  ] as const) {
    Object.defineProperty(globalThis, key, {
      value: (win as never as Record<string, unknown>)[key],
      configurable: true,
    });
  }
}

function setInputValue(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set;
  setter?.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
}

function provider(): ProviderDescriptor {
  return {
    schema_version: 1,
    id: "provider-a",
    display_name: "Provider A",
    transport: "pty",
    system_prompt_delivery: "external",
    available_models: [{ id: "model-a", display_name: "Model A" }],
    default_model: "model-a",
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
  };
}

async function main() {
  setupDom();
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "list_agent_providers") return [provider()] as unknown as T;
    throw new Error(`Unexpected command: ${command}`);
  });
  __setListenForTest(async () => () => {});
  globalThis.fetch = (async () => ({ ok: true, json: async () => ({}) } as Response)) as typeof fetch;

  let currentConfig: AppConfig = defaultConfig();
  currentConfig.general_agent.agent_provider = "provider-a";
  currentConfig.api = undefined as never;
  const updates: Array<Partial<AppConfig["memory"]>> = [];

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);

  const renderTab = async () => {
    await act(async () => {
      root.render(
        <MemorySettingsTab
          config={currentConfig}
          onUpdate={(section, patch) => {
            if (section === "memory") {
              updates.push(patch as Partial<AppConfig["memory"]>);
              currentConfig = {
                ...currentConfig,
                memory: { ...currentConfig.memory, ...(patch as Partial<AppConfig["memory"]>) },
              };
            }
          }}
          onOpenAgentsSettings={() => {}}
        />,
      );
    });
  };

  await renderTab();

  const dedup = host.querySelector('[data-testid="memory-settings-dedup-threshold"]') as HTMLInputElement | null;
  const timeout = host.querySelector('[data-testid="memory-settings-worker-timeout"]') as HTMLInputElement | null;
  const interval = host.querySelector('[data-testid="memory-settings-file-sync-interval"]') as HTMLInputElement | null;
  assert(dedup && timeout && interval, "all three memory numeric settings inputs must render");

  const readDedupValue = () => dedup.value;
  const readThreshold = () => currentConfig.memory.dedup_threshold;
  const updateCount = () => updates.length;

  assert(dedup.type === "text", `dedup threshold must use text input to avoid OS locale formatting, got '${dedup.type}'`);
  assert(dedup.inputMode === "decimal", `dedup threshold inputMode must be decimal, got '${dedup.inputMode}'`);
  assert(readDedupValue() === "0.15", `dedup threshold must display '0.15', got '${readDedupValue()}'`);

  assert(timeout.type === "text" && timeout.inputMode === "numeric", "worker timeout must use numeric text input");
  assert(timeout.value === "1800", `worker timeout must display '1800', got '${timeout.value}'`);
  assert(interval.type === "text" && interval.inputMode === "numeric", "sync interval must use numeric text input");
  assert(interval.value === "60", `sync interval must display '60', got '${interval.value}'`);

  await act(async () => {
    dedup.focus();
    setInputValue(dedup, "0.");
  });
  assert(updateCount() === 0, "incomplete decimal draft must not mutate config");
  assert(readDedupValue() === "0.", `incomplete decimal draft must stay editable, got '${readDedupValue()}'`);

  await act(async () => {
    setInputValue(dedup, "0.25");
  });
  await renderTab();
  assert(updateCount() === 1 && readThreshold() === 0.25, "dot-decimal value must persist as 0.25");
  assert(readDedupValue() === "0.25", `persisted value must round-trip as '0.25', got '${readDedupValue()}'`);

  await act(async () => {
    setInputValue(dedup, "0,35");
  });
  assert(updateCount() === 1 && readThreshold() === 0.25, "locale comma input must not mutate config");
  assert(dedup.getAttribute("aria-invalid") === "true", "locale comma input must mark field invalid");

  await act(async () => {
    dedup.blur();
    dedup.dispatchEvent(new FocusEvent("blur", { bubbles: true }));
  });
  assert(readDedupValue() === "0.25", `blur must restore committed dot-decimal value, got '${readDedupValue()}'`);

  await act(async () => {
    dedup.focus();
    dedup.dispatchEvent(new window.KeyboardEvent("keydown", { key: "ArrowUp", code: "ArrowUp", bubbles: true, cancelable: true }) as unknown as Event);
  });
  await renderTab();
  assert(readThreshold() === 0.26, `ArrowUp must step cleanly to 0.26, got ${readThreshold()} (input.value='${readDedupValue()}', updates=${updateCount()})`);
  assert(readDedupValue() === "0.26", `stepped value must display as '0.26', got '${readDedupValue()}'`);

  await act(async () => root.unmount());
  __setInvokeForTest(null);
  __setListenForTest(null);

  console.log("✓ Memory settings numeric inputs use locale-independent dot-decimal formatting");
}

void main();
