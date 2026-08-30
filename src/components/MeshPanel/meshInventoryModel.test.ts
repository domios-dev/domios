import type { Service } from "./types";
import {
  buildInventoryModel,
  buildInventoryRows,
  classifyInventoryState,
  DEFAULT_INVENTORY_FILTERS,
  inventoryResourceKey,
  sortInventoryRows,
  type InventoryFilters,
  type InventoryPeerIssue,
} from "./meshInventoryModel";

let passed = 0;
let failed = 0;

function assert(label: string, condition: unknown): void {
  if (condition) {
    passed += 1;
    console.log(`  ✓ ${label}`);
  } else {
    failed += 1;
    console.error(`  ✗ ${label}`);
  }
}

function service(overrides: Partial<Service> & Pick<Service, "id" | "name" | "kind">): Service {
  return {
    id: overrides.id,
    name: overrides.name,
    kind: overrides.kind,
    health_mode: "http",
    health_interval: 60,
    health: { status: "healthy" },
    push_enabled: false,
    registered_at: 1,
    updated_at: 1,
    enabled: true,
    ...overrides,
  };
}

const local = service({
  id: "svc-api",
  resource_id: "svc-api",
  name: "API",
  kind: "daemon",
  machine_id: "machine-local",
  machine_name: "Local",
  capabilities: { get: { allowed: true }, delete: { allowed: true } },
});
const remoteFresh = service({
  id: "cron-backup",
  resource_id: "cron-backup",
  name: "Backup",
  kind: "cron",
  machine_id: "machine-remote",
  machine_name: "Remote",
  schedule: "0 3 * * *",
  next_run: 1_800_000_000,
  last_run_status: "success",
  provenance: { freshness: "fresh", offline: false, fetched_at: 1_700_000_000 },
  capabilities: { get: { allowed: true }, trigger: { allowed: true }, delete: { allowed: true } },
});
const remoteStale = service({
  id: "svc-cache",
  resource_id: "svc-cache",
  name: "Cache",
  kind: "daemon",
  machine_id: "machine-remote",
  machine_name: "Remote",
  health: { status: "degraded" },
  stale: true,
  peer_online: false,
  provenance: { freshness: "offline", offline: true, fetched_at: 1_600_000_000 },
  capabilities: { get: { allowed: true }, delete: { allowed: false, unavailable_reason: "offline" } },
});
const denied = service({
  id: "cron-denied",
  resource_id: "cron-denied",
  name: "Locked schedule",
  kind: "cron",
  machine_id: "machine-remote",
  machine_name: "Remote",
  schedule: "*/5 * * * *",
  capabilities: {
    get: { allowed: false, unavailable_reason: "not_authorized" },
    trigger: { allowed: false, unavailable_reason: "not_authorized" },
  },
});
const failedPeer: InventoryPeerIssue = {
  machineId: "machine-failed",
  machineName: "Failed peer",
  freshness: "offline",
  partialError: "peer did not respond",
};

const resources = [remoteStale, denied, local, remoteFresh];
const rows = buildInventoryRows(resources, "Local");

assert("resource keys qualify machine and resource identity", inventoryResourceKey(local) === "machine-local:svc-api");
assert("service and schedule kinds remain separate", rows.find((row) => row.resource.name === "API")?.kindLabel === "Service" && rows.find((row) => row.resource.name === "Backup")?.kindLabel === "Schedule");
assert("stale and offline provenance is preserved", rows.find((row) => row.resource.name === "Cache")?.freshness === "offline");
assert("denied resource has no available operation", rows.find((row) => row.resource.name === "Locked schedule")?.hasAvailableOperation === false);

const byMachine = sortInventoryRows(rows, { key: "machine", direction: "asc" });
assert("machine sort is stable and machine-qualified", byMachine.map((row) => row.resourceKey).join(",") === "machine-local:svc-api,machine-remote:cron-backup,machine-remote:cron-denied,machine-remote:svc-cache");
const bySchedule = sortInventoryRows(rows, { key: "schedule", direction: "asc" });
assert("schedule sort puts the next run first", bySchedule[0]?.resourceKey === "machine-remote:cron-backup");

const matching: InventoryFilters = { ...DEFAULT_INVENTORY_FILTERS, search: "remote backup" };
assert("search matches machine and name", buildInventoryModel({ resources }, matching).rows.map((row) => row.resourceKey).join() === "machine-remote:cron-backup");
assert("type filter selects schedules", buildInventoryModel({ resources }, { ...DEFAULT_INVENTORY_FILTERS, kind: "cron" }).rows.length === 2);
assert("freshness filter selects offline cache", buildInventoryModel({ resources }, { ...DEFAULT_INVENTORY_FILTERS, freshness: "offline" }).rows[0]?.resource.name === "Cache");
assert("capability filter selects denied rows", buildInventoryModel({ resources }, { ...DEFAULT_INVENTORY_FILTERS, capability: "unavailable" }).rows.map((row) => row.resource.name).join() === "Locked schedule");
assert("state filter selects degraded service", buildInventoryModel({ resources }, { ...DEFAULT_INVENTORY_FILTERS, state: "degraded" }).rows[0]?.resource.name === "Cache");

const ready = buildInventoryModel({ resources });
assert("ready state is distinct", ready.state === "ready");
assert("loading with cached rows is distinct", buildInventoryModel({ resources, loading: true }).state === "loading-cached");
assert("loading without rows is distinct", buildInventoryModel({ resources: [], loading: true }).state === "loading");
assert("empty state is distinct", buildInventoryModel({ resources: [] }).state === "empty");
assert("filtered empty is distinct", buildInventoryModel({ resources }, { ...DEFAULT_INVENTORY_FILTERS, search: "missing" }).state === "filtered-empty");
assert("partial peer failure is distinct", buildInventoryModel({ resources: [local], peerIssues: [failedPeer] }).state === "partial-error");
assert("offline rows are honest without peer-id inference", buildInventoryModel({ resources: [remoteStale] }).state === "offline");

const directFiltered = buildInventoryRows([local]);
assert("classify helper accepts deterministic row projections", classifyInventoryState({ resources: [local] }, directFiltered, []) === "ready");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
