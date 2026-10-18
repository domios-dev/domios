import { buildHash } from "./buildStamp";
import type { MultiplexerState, Tab } from "./types";
import {
  frontendCacheRepository,
  frontendCacheScope,
  type FrontendCacheRecord,
  type FrontendCacheSnapshot,
  type FrontendCacheWriteResult,
} from "./frontendCacheRecords";
import { IS_WEB } from "./webApi";

const CACHE_SCOPE = "codeterm:state-cache";
const SCHEMA_VERSION = 2;

type StateCacheInput = MultiplexerState & {
  activeTabIds?: Record<string, string | null | undefined>;
};

interface StateCacheMetadata {
  schemaVersion: number;
  buildHash: string;
  generation: number | null;
  counts: Record<string, number>;
}

const liveCache = new Map<string, { schemaVersion: number; buildHash: string; state: MultiplexerState }>();

function cacheScope(): string | null {
  if (!IS_WEB) return null;
  return frontendCacheScope(CACHE_SCOPE);
}

function createRecord(kind: string, id: string, value: unknown): FrontendCacheRecord {
  return { kind, id, value };
}

function projectRecords(state: StateCacheInput): { records: FrontendCacheRecord[]; counts: Record<string, number> } {
  const records: FrontendCacheRecord[] = [];
  const counts: Record<string, number> = {};
  const push = (kind: string, id: string, value: unknown) => {
    records.push(createRecord(kind, id, value));
    counts[kind] = (counts[kind] ?? 0) + 1;
  };

  state.workspaces.forEach((workspace, position) => push("workspace", workspace.id, { position, workspace }));
  for (const [id, tab] of Object.entries(state.tabs)) push("tab", id, tab);
  for (const [id, status] of Object.entries(state.tab_statuses ?? {})) push("tab-status", id, status);
  (state.teams ?? []).forEach((team, position) => push("team", team.id, { position, team }));
  (state.recently_closed ?? []).forEach((item, position) => push("closed-tab", `${position}:${item.closed_at}:${item.tab_id}`, { position, item }));
  (state.closed_structures ?? []).forEach((item, position) => push("closed-structure", item.id, { position, item }));
  push("selection", "current", {
    active_workspace_id: state.active_workspace_id,
    focused_tab_id: state.focused_tab_id,
    activeTabIds: state.activeTabIds ?? Object.fromEntries(state.workspaces.map((workspace) => [workspace.id, workspace.active_tab_id])),
  });
  return { records, counts };
}

function recordsMatchCounts(snapshot: FrontendCacheSnapshot, metadata: StateCacheMetadata): boolean {
  if (!metadata.counts || typeof metadata.counts !== "object") return false;
  const actual: Record<string, number> = {};
  for (const record of snapshot.records) actual[record.kind] = (actual[record.kind] ?? 0) + 1;
  const kinds = new Set([...Object.keys(actual), ...Object.keys(metadata.counts)]);
  return Array.from(kinds).every((kind) => actual[kind] === metadata.counts[kind]);
}

function positionedValue<T>(record: FrontendCacheRecord, property: string, expectedId: (value: Record<string, unknown>, position: number) => string): { position: number; value: T } | null {
  if (!record.value || typeof record.value !== "object") return null;
  const wrapper = record.value as Record<string, unknown>;
  const value = wrapper[property];
  if (!Number.isInteger(wrapper.position) || (wrapper.position as number) < 0 || !value || typeof value !== "object") return null;
  const row = value as Record<string, unknown>;
  const position = wrapper.position as number;
  if (expectedId(row, position) !== record.id) return null;
  return { position, value: value as T };
}

function orderedValues<T>(rows: Array<{ position: number; value: T }>): T[] | null {
  rows.sort((left, right) => left.position - right.position);
  if (rows.some((row, index) => row.position !== index)) return null;
  return rows.map(({ value }) => value);
}

function stateFromSnapshot(snapshot: FrontendCacheSnapshot): MultiplexerState | null {
  const metadata = snapshot.manifest.metadata as unknown as StateCacheMetadata;
  if (!metadata || metadata.schemaVersion !== SCHEMA_VERSION || metadata.buildHash !== buildHash() || !recordsMatchCounts(snapshot, metadata)) {
    return null;
  }
  const allowedKinds = new Set(["workspace", "tab", "tab-status", "team", "closed-tab", "closed-structure", "selection"]);
  if (snapshot.records.some((record) => !allowedKinds.has(record.kind))) return null;
  const byKind = new Map<string, FrontendCacheRecord[]>();
  for (const record of snapshot.records) {
    const rows = byKind.get(record.kind);
    if (rows) rows.push(record);
    else byKind.set(record.kind, [record]);
  }
  const selectionRecord = byKind.get("selection")?.find((record) => record.id === "current");
  if (!selectionRecord || (byKind.get("selection")?.length ?? 0) !== 1 || !selectionRecord.value || typeof selectionRecord.value !== "object") return null;
  const selection = selectionRecord.value as {
    active_workspace_id: string | null;
    focused_tab_id: string | null;
    activeTabIds?: Record<string, string | null | undefined>;
  };
  if ((selection.active_workspace_id !== null && typeof selection.active_workspace_id !== "string") ||
    (selection.focused_tab_id !== null && typeof selection.focused_tab_id !== "string") ||
    (selection.activeTabIds !== undefined && (!selection.activeTabIds || typeof selection.activeTabIds !== "object" ||
      Object.values(selection.activeTabIds).some((id) => id !== null && typeof id !== "string"))) ||
    (metadata.generation !== null && (!Number.isFinite(metadata.generation) || typeof metadata.generation !== "number"))) return null;
  const workspaceRows = (byKind.get("workspace") ?? []).map((record) =>
    positionedValue<MultiplexerState["workspaces"][number]>(record, "workspace", (row) => String(row.id ?? "")));
  if (workspaceRows.some((row) => !row)) return null;
  const orderedWorkspaces = orderedValues(workspaceRows as Array<{ position: number; value: MultiplexerState["workspaces"][number] }>);
  if (!orderedWorkspaces || orderedWorkspaces.some((workspace) => !Array.isArray(workspace.tabs))) return null;
  const workspaces = orderedWorkspaces.map((workspace) => ({
    ...workspace,
    active_tab_id: selection.activeTabIds && Object.prototype.hasOwnProperty.call(selection.activeTabIds, workspace.id)
      ? selection.activeTabIds[workspace.id] ?? null
      : workspace.active_tab_id,
  }));
  const tabs: Record<string, Tab> = {};
  for (const record of byKind.get("tab") ?? []) {
    if (!record.value || typeof record.value !== "object" || (record.value as Tab).id !== record.id) return null;
    tabs[record.id] = record.value as Tab;
  }
  const tabStatuses: NonNullable<MultiplexerState["tab_statuses"]> = {};
  const statusValues = new Set(["Working", "Waiting", "Idle", "Error", "Dead"]);
  for (const record of byKind.get("tab-status") ?? []) {
    if (typeof record.value !== "string" || !statusValues.has(record.value)) return null;
    tabStatuses[record.id] = record.value as NonNullable<MultiplexerState["tab_statuses"]>[string];
  }
  const teamRows = (byKind.get("team") ?? []).map((record) =>
    positionedValue<NonNullable<MultiplexerState["teams"]>[number]>(record, "team", (row) => String(row.id ?? "")));
  if (teamRows.some((row) => !row)) return null;
  const teams = orderedValues(teamRows as Array<{ position: number; value: NonNullable<MultiplexerState["teams"]>[number] }>);
  if (!teams) return null;
  const closedRows = (byKind.get("closed-tab") ?? []).map((record) =>
    positionedValue<NonNullable<MultiplexerState["recently_closed"]>[number]>(record, "item", (row, position) => `${position}:${row.closed_at}:${row.tab_id}`));
  if (closedRows.some((row) => !row)) return null;
  const recently_closed = orderedValues(closedRows as Array<{ position: number; value: NonNullable<MultiplexerState["recently_closed"]>[number] }>);
  if (!recently_closed) return null;
  const structureRows = (byKind.get("closed-structure") ?? []).map((record) =>
    positionedValue<NonNullable<MultiplexerState["closed_structures"]>[number]>(record, "item", (row) => String(row.id ?? "")));
  if (structureRows.some((row) => !row)) return null;
  const closed_structures = orderedValues(structureRows as Array<{ position: number; value: NonNullable<MultiplexerState["closed_structures"]>[number] }>);
  if (!closed_structures) return null;

  return {
    workspaces,
    tabs,
    teams,
    active_workspace_id: selection.active_workspace_id,
    focused_tab_id: selection.focused_tab_id,
    generation: metadata.generation ?? undefined,
    recently_closed,
    closed_structures,
    tab_statuses: tabStatuses,
  };
}

function metadataFor(state: StateCacheInput, counts: Record<string, number>): StateCacheMetadata {
  return {
    schemaVersion: SCHEMA_VERSION,
    buildHash: buildHash(),
    generation: typeof state.generation === "number" ? state.generation : null,
    counts,
  };
}

export function loadCachedStateSync(): MultiplexerState | null {
  const scope = cacheScope();
  const cached = scope ? liveCache.get(scope) : undefined;
  return cached?.schemaVersion === SCHEMA_VERSION && cached.buildHash === buildHash() ? cached.state : null;
}

export async function loadCachedStateAsync(): Promise<MultiplexerState | null> {
  const scope = cacheScope();
  if (!scope) return null;
  try {
    const snapshot = await frontendCacheRepository.read(scope);
    if (cacheScope() !== scope) return null;
    if (!snapshot) return null;
    const state = stateFromSnapshot(snapshot);
    if (!state) {
      await frontendCacheRepository.clear(scope, snapshot.manifest.revision);
      liveCache.delete(scope);
      return null;
    }
    liveCache.set(scope, { schemaVersion: SCHEMA_VERSION, buildHash: buildHash(), state });
    return state;
  } catch {
    return null;
  }
}

export function saveStateSnapshot(state: StateCacheInput): Promise<FrontendCacheWriteResult | null> {
  const scope = cacheScope();
  if (!scope) return Promise.resolve(null);
  liveCache.set(scope, { schemaVersion: SCHEMA_VERSION, buildHash: buildHash(), state });
  const { records, counts } = projectRecords(state);
  const metadata = metadataFor(state, counts) as unknown as Record<string, unknown>;
  return frontendCacheRepository.write(scope, records, metadata).catch(() => null);
}

export function clearStateSnapshot(): Promise<void> {
  const scope = cacheScope();
  if (!scope) return Promise.resolve();
  liveCache.delete(scope);
  return frontendCacheRepository.clear(scope).then(() => undefined).catch(() => undefined);
}

export interface WriteGate {
  canWrite(): boolean;
  lock(): void;
  unlock(): void;
}

export function createWriteGate(locked = false): WriteGate {
  let isLocked = locked;
  return {
    canWrite: () => !isLocked,
    lock: () => { isLocked = true; },
    unlock: () => { isLocked = false; },
  };
}
