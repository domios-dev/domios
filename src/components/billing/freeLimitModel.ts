import { TAB_LIMIT_NOUN } from "./limitCopy";
import type { MultiplexerState } from "../../lib/types";

/** Stable daemon refusal code for a free-tier limit. */
export const FREE_LIMIT_REACHED = "free_limit_reached";

/** Limits the daemon counts on this machine. */
export type LocalLimitKey = "workspaces" | "teams" | "agent.tabs";

/** `mesh.machines` is counted by the account server only, so it is never pre-checked or read from daemon text. */
export type FreeLimitKey = LocalLimitKey | "mesh.machines";

export interface FreeLimits {
  enforcing: boolean;
  max: Record<LocalLimitKey, number | null>;
}

export type LimitCounts = Record<LocalLimitKey, number>;

/** The server's own count for a refused Mesh machine: `activeCount` active slots out of `limit`, this machine included in `limit`. */
export interface MeshMachinesLimit {
  limit: number;
  activeCount: number;
}

const KEYS: readonly LocalLimitKey[] = ["workspaces", "teams", "agent.tabs"];
const COMPILED_FREE_MAX: Record<FreeLimitKey, number> = { workspaces: 2, teams: 6, "agent.tabs": 35, "mesh.machines": 2 };
const MESH_MACHINES: FreeLimitKey = "mesh.machines";

const isKey = (value: string): value is LocalLimitKey => (KEYS as readonly string[]).includes(value);
const isCount = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const isMax = (value: unknown): value is number | null =>
  value === null || (typeof value === "number" && Number.isInteger(value) && value >= 0);

/** The plan payload's `limits`; null when absent or malformed, which disables the pre-check. */
export function parseFreeLimits(value: unknown): FreeLimits | null {
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  if (typeof record.enforcing !== "boolean") return null;
  const max = {} as FreeLimits["max"];
  for (const key of KEYS) {
    const entry = record[key] ?? null;
    if (!isMax(entry)) return null;
    max[key] = entry;
  }
  return { enforcing: record.enforcing, max };
}

/** Local items only, mirroring the daemon: the protected General workspace and peer projections never count. */
export function limitCounts(state: Pick<MultiplexerState, "workspaces" | "tabs" | "teams">): LimitCounts {
  return {
    workspaces: state.workspaces.filter((workspace) => !workspace.protected).length,
    teams: (state.teams ?? []).filter((team) => !team.remote_peer_name).length,
    "agent.tabs": Object.values(state.tabs).filter((tab) => !tab.remote_peer_id && !tab.remote_tab_id).length,
  };
}

/** The first of `keys` already full; null unless the daemon reports enforcement on. */
export function reachedLimit(limits: FreeLimits | null, counts: LimitCounts, keys: readonly LocalLimitKey[]): LocalLimitKey | null {
  if (!limits?.enforcing) return null;
  return keys.find((key) => {
    const max = limits.max[key];
    return max !== null && counts[key] >= max;
  }) ?? null;
}

const normalizePath = (path: string) => path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();

/** The daemon reuses a workspace with the same folder instead of creating one; lenient so it never blocks a reuse. */
export function reusesWorkspace(workspaces: MultiplexerState["workspaces"], cwd: string | undefined): boolean {
  if (!cwd) return false;
  const target = normalizePath(cwd);
  return workspaces.some((workspace) => workspace.cwd != null && normalizePath(workspace.cwd) === target);
}

/** The limit a daemon refusal names, `fallback` when it predates the key, null for any other error or a Mesh limit. */
export function refusedLimit(error: unknown, fallback: LocalLimitKey): LocalLimitKey | null {
  const text = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  if (!text.includes(`${FREE_LIMIT_REACHED}:`)) return null;
  const named = /\(limit: ([a-z.]+)\)/.exec(text)?.[1];
  if (named === MESH_MACHINES) return null;
  return named && isKey(named) ? named : fallback;
}

/** The server's structured `mesh.machines` refusal; null for every other refusal, outage, or inconsistent count. */
export function meshMachinesRefusal(refusal: unknown): MeshMachinesLimit | null {
  if (typeof refusal !== "object" || refusal === null || Array.isArray(refusal)) return null;
  const { reason, limit, active_count: activeCount } = refusal as Record<string, unknown>;
  if (reason !== MESH_MACHINES || !isCount(limit) || limit === 0 || !isCount(activeCount) || activeCount < limit) return null;
  return { limit, activeCount };
}

const LOCKED_NOUN: Record<LocalLimitKey, string> = { workspaces: "Workspace", teams: "Team", "agent.tabs": "Tab" };

/** What a row shows for a plan-limit lock: a locked item stays listed and names the limit its click opens. */
export type LockedView = { locked: false } | { locked: true; limit: LocalLimitKey; label: string };

const UNLOCKED: LockedView = { locked: false };

/** Reads the daemon's `locked` mark on a workspace, tab, or Team; the daemon is the only authority. */
export function lockedView(limit: LocalLimitKey, item: { locked?: boolean } | null | undefined): LockedView {
  return item?.locked === true ? { locked: true, limit, label: `${LOCKED_NOUN[limit]} locked by the free plan` } : UNLOCKED;
}

/** A tab inherits the workspace and Team lock marks. */
export function tabLockedView(tab: { locked?: boolean } | null | undefined, workspace: { locked?: boolean } | null | undefined, team?: { locked?: boolean } | null): LockedView {
  const own = lockedView("agent.tabs", tab);
  if (own.locked) return own;
  if (workspace?.locked === true) return { locked: true, limit: "workspaces", label: "Tab locked: its workspace is over the free plan" };
  if (team?.locked === true) return { locked: true, limit: "teams", label: "Tab locked: its Team is over the free plan" };
  return own;
}

/** Muted look for a locked item's title and icon; the lock badge itself is never dimmed. */
export const lockedDimClass = (view: { locked: boolean }): string => (view.locked ? "opacity-60" : "");

export const isFreeLimitRefusal = (error: unknown): boolean => refusedLimit(error, "workspaces") !== null;

/** The same coded shape the daemon returns, for a creation the client refused before calling it. */
export function freeLimitBlocked(key: FreeLimitKey): Error {
  return new Error(`${FREE_LIMIT_REACHED}: refused before creation (limit: ${key})`);
}

export function limitHeadline(key: FreeLimitKey, max?: number | null): string {
  const count = max ?? COMPILED_FREE_MAX[key];
  switch (key) {
    case "workspaces": return `The free version includes ${count} workspaces.`;
    case "teams": return `The free version includes ${count} Teams across all workspaces.`;
    case "agent.tabs": return `The free version allows ${count} ${TAB_LIMIT_NOUN}.`;
    case "mesh.machines": return `The free version includes ${count} machines in your mesh.`;
  }
}

export function meshMachinesDetail({ activeCount }: MeshMachinesLimit): string {
  return `This machine counts toward that limit, and your account already has ${activeCount} active machines. `
    + "Revoking a machine from your account frees its slot; signing out or unpairing locally does not.";
}
