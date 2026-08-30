import type { InventoryOperation, Service } from "./types";

export type InventoryKind = "cron" | "daemon";
export type InventoryState = "healthy" | "degraded" | "down" | "unknown" | "disabled" | "running" | "failed" | "idle";
export type InventoryFreshness = "fresh" | "stale" | "offline";
export type InventorySortKey = "machine" | "name" | "kind" | "state" | "schedule";
export type InventoryCapabilityFilter = "all" | "available" | "unavailable";

export interface InventoryFilters {
  search: string;
  machine: string;
  kind: "all" | InventoryKind;
  state: "all" | InventoryState;
  freshness: "all" | InventoryFreshness;
  capability: InventoryCapabilityFilter;
}

export interface InventorySort {
  key: InventorySortKey;
  direction: "asc" | "desc";
}

export interface InventoryPeerIssue {
  machineId: string;
  machineName: string;
  freshness: InventoryFreshness;
  partialError?: string | null;
}

export interface InventorySnapshot {
  resources: Service[];
  peerIssues?: InventoryPeerIssue[];
  loading?: boolean;
  error?: string | null;
}

export interface InventoryRow {
  resource: Service;
  resourceKey: string;
  resourceId: string;
  machineId: string;
  machineName: string;
  kind: InventoryKind;
  kindLabel: "Schedule" | "Service";
  state: InventoryState;
  stateLabel: string;
  freshness: InventoryFreshness;
  schedule: string | null;
  nextRun: number | null;
  lastRunResult: string | null;
  availableOperations: InventoryOperation[];
  hasAvailableOperation: boolean;
  operationSummary: string;
  searchText: string;
}

export type InventoryViewState = "loading" | "loading-cached" | "empty" | "filtered-empty" | "ready" | "stale" | "offline" | "partial-error" | "error";

export interface InventoryModel {
  allRows: InventoryRow[];
  rows: InventoryRow[];
  peerIssues: InventoryPeerIssue[];
  state: InventoryViewState;
  total: number;
  visible: number;
}

const OPERATION_LABELS: Record<InventoryOperation, string> = {
  get: "inspect",
  history: "history",
  update_schedule: "schedule",
  delete: "delete",
  enable: "enable",
  disable: "disable",
  trigger: "run now",
  cancel: "cancel",
};

const STATE_RANK: Record<InventoryState, number> = {
  down: 0,
  failed: 1,
  degraded: 2,
  unknown: 3,
  disabled: 4,
  running: 5,
  idle: 6,
  healthy: 7,
};

const KIND_RANK: Record<InventoryKind, number> = { cron: 0, daemon: 1 };

function compareText(left: string, right: string): number {
  const a = left.toLowerCase();
  const b = right.toLowerCase();
  return a < b ? -1 : a > b ? 1 : 0;
}

export const DEFAULT_INVENTORY_FILTERS: InventoryFilters = {
  search: "",
  machine: "all",
  kind: "all",
  state: "all",
  freshness: "all",
  capability: "all",
};

export const DEFAULT_INVENTORY_SORT: InventorySort = {
  key: "machine",
  direction: "asc",
};

export function inventoryResourceKey(resource: Service): string {
  const machineId = resource.machine_id ?? resource.source_peer_id ?? resource.machine_name ?? resource.source_peer ?? "local";
  const resourceId = resource.resource_id ?? resource.id;
  return `${machineId}:${resourceId}`;
}

export function inventoryFreshness(resource: Service): InventoryFreshness {
  if (resource.provenance?.freshness) return resource.provenance.freshness;
  if (resource.peer_online === false) return "offline";
  return resource.stale ? "stale" : "fresh";
}

export function inventoryState(resource: Service): InventoryState {
  if (resource.kind === "cron") {
    if (!resource.enabled) return "disabled";
    if (resource.last_run_status === "running") return "running";
    if (resource.last_run_status === "failed") return "failed";
    return "idle";
  }
  return resource.health.status;
}

function operationSummary(resource: Service): { operations: InventoryOperation[]; label: string } {
  const operations = (Object.entries(resource.capabilities ?? {}) as [InventoryOperation, { allowed: boolean }][])
    .filter(([, capability]) => capability.allowed)
    .map(([operation]) => operation)
    .sort();
  if (operations.length === 0) return { operations, label: "No operations available" };
  return { operations, label: operations.map((operation) => OPERATION_LABELS[operation]).join(", ") };
}

export function buildInventoryRows(resources: Service[], thisMachineName = "This machine"): InventoryRow[] {
  return resources.map((resource) => {
    const machineId = resource.machine_id ?? resource.source_peer_id ?? "local";
    const machineName = resource.machine_name ?? resource.source_peer ?? thisMachineName;
    const state = inventoryState(resource);
    const freshness = inventoryFreshness(resource);
    const operations = operationSummary(resource);
    const kind = resource.kind;
    const kindLabel = kind === "cron" ? "Schedule" : "Service";
    const lastRunResult = kind === "cron" ? resource.last_run_status ?? null : null;
    const schedule = resource.schedule ?? null;
    const nextRun = resource.next_run ?? null;
    const searchText = [
      resource.name,
      resource.id,
      resource.resource_id,
      machineId,
      machineName,
      kindLabel,
      kind,
      state,
      resource.health.status,
      schedule,
      lastRunResult,
      freshness,
      operations.label,
    ].filter(Boolean).join(" ").toLowerCase();
    return {
      resource,
      resourceKey: inventoryResourceKey(resource),
      resourceId: resource.resource_id ?? resource.id,
      machineId,
      machineName,
      kind,
      kindLabel,
      state,
      stateLabel: state,
      freshness,
      schedule,
      nextRun,
      lastRunResult,
      availableOperations: operations.operations,
      hasAvailableOperation: operations.operations.length > 0,
      operationSummary: operations.label,
      searchText,
    };
  });
}

export function filterInventoryRows(rows: InventoryRow[], filters: InventoryFilters): InventoryRow[] {
  const search = filters.search.trim().toLowerCase();
  return rows.filter((row) => {
    if (search && !row.searchText.includes(search)) return false;
    if (filters.machine !== "all" && row.machineId !== filters.machine) return false;
    if (filters.kind !== "all" && row.kind !== filters.kind) return false;
    if (filters.state !== "all" && row.state !== filters.state) return false;
    if (filters.freshness !== "all" && row.freshness !== filters.freshness) return false;
    if (filters.capability === "available" && !row.hasAvailableOperation) return false;
    if (filters.capability === "unavailable" && row.hasAvailableOperation) return false;
    return true;
  });
}

function compareNullableNumber(left: number | null, right: number | null): number {
  if (left == null && right == null) return 0;
  if (left == null) return 1;
  if (right == null) return -1;
  return left - right;
}

export function sortInventoryRows(rows: InventoryRow[], sort: InventorySort): InventoryRow[] {
  return rows
    .map((row, index) => ({ row, index }))
    .sort((left, right) => {
      let result = 0;
      switch (sort.key) {
        case "machine":
          result = compareText(left.row.machineName, right.row.machineName) || compareText(left.row.machineId, right.row.machineId);
          break;
        case "name":
          result = compareText(left.row.resource.name, right.row.resource.name);
          break;
        case "kind":
          result = KIND_RANK[left.row.kind] - KIND_RANK[right.row.kind];
          break;
        case "state":
          result = STATE_RANK[left.row.state] - STATE_RANK[right.row.state];
          break;
        case "schedule":
          result = compareNullableNumber(left.row.nextRun, right.row.nextRun);
          break;
      }
      if (sort.direction === "desc") result *= -1;
      return result || compareText(left.row.resourceKey, right.row.resourceKey) || left.index - right.index;
    })
    .map(({ row }) => row);
}

export function classifyInventoryState(snapshot: InventorySnapshot, allRows: InventoryRow[], visibleRows: InventoryRow[]): InventoryViewState {
  if (snapshot.loading && allRows.length === 0) return "loading";
  if (snapshot.error && allRows.length === 0) return "error";
  if (allRows.length === 0) return snapshot.peerIssues?.length ? "partial-error" : "empty";
  if (visibleRows.length === 0) return "filtered-empty";
  if (snapshot.error) return "partial-error";
  if (snapshot.peerIssues?.length) return "partial-error";
  if (allRows.some((row) => row.freshness === "offline")) return "offline";
  if (allRows.some((row) => row.freshness === "stale")) return "stale";
  return snapshot.loading ? "loading-cached" : "ready";
}

export function buildInventoryModel(
  snapshot: InventorySnapshot,
  filters: InventoryFilters = DEFAULT_INVENTORY_FILTERS,
  sort: InventorySort = DEFAULT_INVENTORY_SORT,
  thisMachineName = "This machine",
): InventoryModel {
  const allRows = buildInventoryRows(snapshot.resources, thisMachineName);
  const rows = sortInventoryRows(filterInventoryRows(allRows, filters), sort);
  return {
    allRows,
    rows,
    peerIssues: snapshot.peerIssues ?? [],
    state: classifyInventoryState(snapshot, allRows, rows),
    total: allRows.length,
    visible: rows.length,
  };
}
