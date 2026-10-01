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
  for (const key of ["window", "document", "HTMLElement", "SVGElement", "Element", "Node", "Event"] as const) {
    Object.defineProperty(globalThis, key, { value: (window as never as Record<string, unknown>)[key], configurable: true });
  }
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

const ENABLED_CRON = [
  {
    id: "cid",
    name: "mem-dreaming-auto",
    description: null,
    workspace_id: "",
    schedule: "0 6 * * *",
    enabled: true,
    agent: "claude",
    model: null,
    task: "",
    cwd: null,
    role: null,
    oneshot: false,
    max_history: 5,
    created_at: 0,
    last_run: null,
    next_run: 1785910139624,
    jitter_max_ms: null,
  },
];

async function main() {
  setupDom();
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "list_agent_providers") return [provider()] as unknown as T;
    throw new Error(`Unexpected command: ${command}`);
  });
  __setListenForTest(async () => () => {});

  let cronFetched = false;
  globalThis.fetch = (async (url: string | URL) => {
    const u = String(url);
    if (u.includes("/api/crons")) {
      cronFetched = true;
      return { ok: true, json: async () => ENABLED_CRON } as Response;
    }
    if (u.includes("/api/mem/sync/status")) return { ok: true, json: async () => ({ tracked_files: 0, stale_files: 0 }) } as Response;
    return { ok: true, json: async () => ({}) } as Response;
  }) as typeof fetch;

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const config = defaultConfig();
  config.general_agent.agent_provider = "provider-a";
  // The desktop app can render this tab before `get_config` populates `api`
  // (the existing model-selector test exercises the same undefined state).
  config.api = undefined as never;

  await act(async () => {
    root.render(<MemorySettingsTab config={config} onUpdate={() => {}} onOpenAgentsSettings={() => {}} />);
  });
  await act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); });

  const schedule = host.querySelector('[data-testid="memory-settings-dream-schedule"]') as HTMLSelectElement | null;
  assert(schedule, "schedule dropdown renders");
  console.log("cron fetched:", cronFetched, "| dropdown value:", schedule.value, "| disabled:", schedule.disabled);

  // With an enabled mem-dreaming-auto cron reachable at the default daemon,
  // the schedule must mirror it (Daily / 06:00), never stay Off.
  assert(cronFetched, "cron endpoint must be queried even when config.api is not yet populated");
  assert(schedule.value === "daily", `schedule must reflect enabled cron (daily), got '${schedule.value}'`);
  assert(!schedule.disabled, "schedule must be enabled when the cron exists");

  const time = host.querySelector('[data-testid="memory-settings-dream-time"]') as HTMLInputElement | null;
  assert(time && time.value === "06:00", `time must be 06:00, got '${time?.value}'`);

  await act(async () => root.unmount());
  __setInvokeForTest(null);
  __setListenForTest(null);
  console.log("✓ Dreaming schedule dropdown mirrors the enabled cron");
}

void main();
