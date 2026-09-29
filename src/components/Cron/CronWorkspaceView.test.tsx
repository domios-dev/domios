// Run: node scripts/run-ts-unit-tests.mjs src/components/Cron/CronWorkspaceView.test.tsx

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import type { CronJob, CronRunInfo } from "../../lib/types";
import type { MeshInventoryView } from "../MeshPanel/servicesView";
import type { InventoryOperation, Service } from "../MeshPanel/types";
import { capabilityFor, capabilityReason, type MeshResourceActionAdapter } from "../MeshPanel/meshResourceActions";
import { CronWorkspaceView, selectCronService } from "./CronWorkspaceView";

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
  const window = new Window({ url: "http://127.0.0.1/cron" });
  for (const key of ["window", "document", "HTMLElement", "HTMLButtonElement", "HTMLInputElement", "navigator", "Element", "Node", "Event"]) {
    Object.defineProperty(globalThis, key, {
      value: (window as unknown as Record<string, unknown>)[key],
      configurable: true,
    });
  }
}

const CRON_ID = "cron-1";

// Mirrors the fully allowed cron projection returned by mesh_inventory_list.
// The component must consume this map rather than deriving its own.
function cronJob(): CronJob {
  return {
    id: CRON_ID,
    name: "Nightly sync",
    description: null,
    workspace_id: "team_x",
    team_id: "team_x",
    schedule: "*/5 * * * *",
    enabled: true,
    agent: "claude",
    model: null,
    reasoning_effort: null,
    task: "sync notes",
    cwd: null,
    role: null,
    report_target_tab_id: null,
    oneshot: false,
    max_history: 10,
    created_at: 1,
    last_run: { run_id: "run-1", started_at: 1, finished_at: 2, status: "Success", tab_id: null },
    next_run: null,
    jitter_max_ms: null,
  };
}

function projectedCronService(lastRunStatus = "Success"): Service {
  return {
    id: `cron_${CRON_ID}`,
    name: "Nightly sync",
    kind: "cron",
    health_mode: "http",
    health_interval: 0,
    health: { status: "healthy" },
    push_enabled: false,
    schedule: "*/5 * * * *",
    last_run_status: lastRunStatus,
    registered_at: 1,
    updated_at: 1,
    enabled: true,
    resource_id: `cron_${CRON_ID}`,
    machine_id: "machine-local",
    machine_name: "This machine",
    provenance: { freshness: "fresh", offline: false, fetched_at: 1 },
    capabilities: {
      get: { allowed: true },
      history: { allowed: true },
      update_schedule: { allowed: true },
      delete: { allowed: true },
      enable: { allowed: true },
      disable: { allowed: true },
      trigger: { allowed: true },
      cancel: { allowed: true },
    },
  };
}

function inventory(resources: Service[]): MeshInventoryView {
  return {
    contract_version: 1,
    machine_id: "machine-local",
    machine_name: "This machine",
    resources,
    provenance: { freshness: "fresh", offline: false, fetched_at: 1 },
  };
}

const FULLY_CAPABLE_OPERATIONS: InventoryOperation[] = [
  "history",
  "get",
  "trigger",
  "enable",
  "disable",
  "update_schedule",
];

test("a resource selected from the mesh inventory projection carries full cron capabilities", () => {
  const resource = selectCronService(inventory([projectedCronService()]), CRON_ID);
  assert(resource, "resource is found in the projected inventory");
  for (const operation of FULLY_CAPABLE_OPERATIONS) {
    assert(capabilityFor(resource, operation).allowed === true, `${operation} is allowed`);
  }
  assert(capabilityReason(resource, "history") === undefined, "history carries no unavailable reason");
});

function adapter(overrides: Partial<MeshResourceActionAdapter> = {}, calls: string[] = []): MeshResourceActionAdapter {
  return {
    get: async () => { calls.push("get"); return cronJob(); },
    history: async () => { calls.push("history"); return [] as CronRunInfo[]; },
    trigger: async () => ({ status: "Running" }),
    setEnabled: async () => ({ enabled: true }),
    cancel: async () => ({ status: "Cancelled" }),
    updateSchedule: async () => ({ schedule: "*/5 * * * *" }),
    remove: async () => undefined,
    regenerateToken: async () => "token",
    ...overrides,
  };
}

/** Yields microtask ticks until `predicate` holds; the cap is a deadlock guard, not the success signal. */
async function flushUntil(predicate: () => boolean, maxTicks = 50): Promise<void> {
  for (let tick = 0; tick < maxTicks && !predicate(); tick++) {
    await Promise.resolve();
  }
  assert(predicate(), `condition observed within ${maxTicks} microtask ticks`);
}

async function render(cronId: string, actionAdapter: MeshResourceActionAdapter, calls: string[], lastRunStatus = "Success"): Promise<{ host: HTMLDivElement; root: Root }> {
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "cron_get") return cronJob() as T;
    if (command === "mesh_inventory_list") return inventory([projectedCronService(lastRunStatus)]) as T;
    throw new Error(`unexpected invoke: ${command}`);
  });
  __setListenForTest(async () => () => undefined);
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<CronWorkspaceView cronId={cronId} actionAdapter={actionAdapter} />);
  });
  // Wait for the cron_get + mesh_inventory_list refresh, then the CronDrawerSection
  // mount it unlocks, to actually invoke the adapter — not a fixed tick count.
  await act(async () => {
    await flushUntil(() => calls.includes("get") && calls.includes("history"));
  });
  return { host, root };
}

test("a cron with no report_target_tab_id renders schedule, history and controls with nothing unavailable", async () => {
  const calls: string[] = [];
  const { host, root } = await render(CRON_ID, adapter({}, calls), calls);

  const unavailable = host.querySelectorAll('[data-testid^="mesh-operation-unavailable-"]');
  assert(unavailable.length === 0, `no unavailable operations rendered, found ${unavailable.length}`);

  assert(calls.includes("history"), "history() was called because can(\"history\") is true");
  assert(calls.filter((c) => c === "history").length === 1, "history() was called exactly once");

  const triggerButton = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent?.includes("Trigger now"));
  const pauseButton = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent?.includes("Pause") || b.textContent?.includes("Resume"));
  assert(triggerButton != null, "Trigger control is present");
  assert(pauseButton != null, "Pause/Resume control is present");
  assert(triggerButton.disabled === false, "Trigger control is not disabled");
  assert(pauseButton.disabled === false, "Pause/Resume control is not disabled");

  await act(async () => root.unmount());
});

test("an owner prompt remains visible in history and exposes cancel while active", async () => {
  const calls: string[] = [];
  const { host, root } = await render(CRON_ID, adapter({
    history: async () => {
      calls.push("history");
      return [{
        run_id: "run-blocked",
        started_at: 100,
        finished_at: null,
        status: "AwaitingOwnerPrompt",
        tab_id: "cron-tab",
      }];
    },
  }, calls), calls, "AwaitingOwnerPrompt");

  assert(host.textContent?.includes("Awaiting owner"), "history labels the blocked run as awaiting its owner");
  assert(host.textContent?.includes("!"), "history uses the owner-prompt status indicator");
  const cancelButton = Array.from(host.querySelectorAll<HTMLButtonElement>("button"))
    .find((button) => button.textContent?.includes("Cancel running"));
  assert(cancelButton, "the blocked active run exposes its cancel control");

  await act(async () => root.unmount());
});

async function main() {
  setupDom();
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (error) {
      console.error(`  ✗ ${name}`);
      console.error(error instanceof Error ? error.stack : error);
      process.exit(1);
    }
  }
  __setInvokeForTest(null);
  __setListenForTest(null);
  console.log(`\n✓ ${passed}/${tests.length} CronWorkspaceView tests passed`);
}

main();
