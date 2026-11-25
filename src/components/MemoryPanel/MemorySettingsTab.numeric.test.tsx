// Run: npx tsx src/components/MemoryPanel/MemorySettingsTab.numeric.test.tsx

import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import type { AppConfig, ProviderDescriptor } from "../../lib/types";
import { defaultConfig, useConfigLoader } from "../../hooks/useConfig";
import { MemorySettingsTab } from "./MemorySettingsTab";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function accessibleNameOf(input: HTMLInputElement): string {
  const ariaLabel = input.getAttribute("aria-label")?.trim();
  if (ariaLabel) return ariaLabel;
  if (input.id) {
    const label = document.querySelector(`label[for="${input.id}"]`);
    if (label?.textContent?.trim()) return label.textContent.trim();
  }
  const wrapping = input.closest("label");
  if (wrapping) {
    const clone = wrapping.cloneNode(true) as HTMLElement;
    for (const control of Array.from(clone.querySelectorAll("input, select, textarea"))) control.remove();
    if (clone.textContent?.trim()) return clone.textContent.trim();
  }
  return "";
}

function pressArrow(input: HTMLInputElement, key: "ArrowUp" | "ArrowDown") {
  input.dispatchEvent(
    new window.KeyboardEvent("keydown", { key, code: key, bubbles: true, cancelable: true }) as unknown as Event,
  );
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
  assert(accessibleNameOf(dedup) === "Dedup threshold", `dedup threshold must have accessible name 'Dedup threshold', got '${accessibleNameOf(dedup)}'`);

  assert(timeout.type === "text" && timeout.inputMode === "numeric", "worker timeout must use numeric text input");
  assert(timeout.value === "1800", `worker timeout must display '1800', got '${timeout.value}'`);
  assert(accessibleNameOf(timeout) === "Worker timeout (s)", `worker timeout must have accessible name 'Worker timeout (s)', got '${accessibleNameOf(timeout)}'`);
  assert(interval.type === "text" && interval.inputMode === "numeric", "sync interval must use numeric text input");
  assert(interval.value === "60", `sync interval must display '60', got '${interval.value}'`);
  assert(accessibleNameOf(interval) === "Interval (min)", `sync interval must have accessible name 'Interval (min)', got '${accessibleNameOf(interval)}'`);

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
    pressArrow(dedup, "ArrowUp");
  });
  await renderTab();
  assert(readThreshold() === 0.26, `ArrowUp must step cleanly to 0.26, got ${readThreshold()} (input.value='${readDedupValue()}', updates=${updateCount()})`);
  assert(readDedupValue() === "0.26", `stepped value must display as '0.26', got '${readDedupValue()}'`);

  await act(async () => root.unmount());
  host.remove();

  // End-to-end burst with real useConfigLoader serialization + delayed config-changed echoes.
  let savedConfig: AppConfig = defaultConfig();
  savedConfig.general_agent.agent_provider = "provider-a";
  savedConfig.memory.dedup_threshold = 0.2;
  let configChanged: ((event: { payload: AppConfig }) => void) | undefined;
  const pendingWrites: Array<{ threshold: number; resolve: () => void }> = [];
  let maxConcurrentWrites = 0;
  let activeWrites = 0;

  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "list_agent_providers") return [provider()] as unknown as T;
    if (command === "get_config") return structuredClone(savedConfig) as unknown as T;
    if (command === "update_config") {
      activeWrites += 1;
      maxConcurrentWrites = Math.max(maxConcurrentWrites, activeWrites);
      const next = structuredClone(args?.config as AppConfig);
      await new Promise<void>((resolve) => {
        pendingWrites.push({
          threshold: next.memory.dedup_threshold,
          resolve: () => {
            savedConfig = next;
            activeWrites -= 1;
            configChanged?.({ payload: structuredClone(savedConfig) });
            resolve();
          },
        });
      });
      return undefined as T;
    }
    throw new Error(`Unexpected command: ${command}`);
  });
  __setListenForTest(async <T,>(event: string, handler: (event: { payload: T }) => void) => {
    if (event === "config-changed") {
      configChanged = (payload) => handler(payload as unknown as { payload: T });
    }
    return () => {
      if (event === "config-changed") configChanged = undefined;
    };
  });

  function LiveMemorySettingsHarness() {
    const { config, updateConfig } = useConfigLoader(true);
    return (
      <MemorySettingsTab
        config={config}
        onUpdate={(section, patch) => {
          const next = { ...config, [section]: Object.assign({}, config[section], patch) };
          void updateConfig(next).catch(() => {});
        }}
        onOpenAgentsSettings={() => {}}
      />
    );
  }

  const liveHost = document.createElement("div");
  document.body.appendChild(liveHost);
  const liveRoot = createRoot(liveHost);
  await act(async () => {
    liveRoot.render(<LiveMemorySettingsHarness />);
  });
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });

  const liveDedup = liveHost.querySelector('[data-testid="memory-settings-dedup-threshold"]') as HTMLInputElement | null;
  assert(liveDedup && liveDedup.value === "0.2", `live dedup must start at '0.2', got '${liveDedup?.value}'`);

  await act(async () => {
    liveDedup.focus();
    liveDedup.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
    pressArrow(liveDedup, "ArrowDown");
    pressArrow(liveDedup, "ArrowDown");
  });
  assert(liveDedup.value === "0.18", `two ArrowDowns must show '0.18', got '${liveDedup.value}'`);
  assert(pendingWrites.length === 1 && pendingWrites[0].threshold === 0.19, "only first write is in flight while second queues");

  // First write settles and echoes 0.19 mid-burst.
  await act(async () => {
    pendingWrites.shift()!.resolve();
    await Promise.resolve();
  });
  assert(liveDedup.value === "0.18", `delayed 0.19 echo must not overwrite focused draft '0.18', got '${liveDedup.value}'`);

  await act(async () => {
    pressArrow(liveDedup, "ArrowDown");
    pressArrow(liveDedup, "ArrowDown");
    pressArrow(liveDedup, "ArrowDown");
  });
  assert(liveDedup.value === "0.15", `five ArrowDowns from 0.20 must reach '0.15', got '${liveDedup.value}'`);

  // Drain all remaining queued writes and echoes.
  while (pendingWrites.length > 0) {
    await act(async () => {
      pendingWrites.shift()!.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
  }

  assert(maxConcurrentWrites === 1, `config writes must be serialized, observed maxConcurrentWrites=${maxConcurrentWrites}`);
  assert(savedConfig.memory.dedup_threshold === 0.15, `final saved dedup_threshold must be 0.15, got ${savedConfig.memory.dedup_threshold}`);
  assert(liveDedup.value === "0.15", `final displayed dedup_threshold must be '0.15', got '${liveDedup.value}'`);

  await act(async () => liveRoot.unmount());
  liveHost.remove();
  __setInvokeForTest(null);
  __setListenForTest(null);

  console.log("✓ Memory settings numeric inputs preserve dot-decimal formatting, accessible names, and rapid key bursts");
}

void main();
