import type { MultiplexerState } from "../../lib/types";

/** Stable daemon refusal code for a free-tier limit. */
export const FREE_LIMIT_REACHED = "free_limit_reached";

export type FreeLimitKey = "workspaces" | "teams" | "agent.tabs";

export interface FreeLimits {
  enforcing: boolean;
  max: Record<FreeLimitKey, number | null>;
}

export type LimitCounts = Record<FreeLimitKey, number>;

const KEYS: readonly FreeLimitKey[] = ["workspaces", "teams", "agent.tabs"];
const COMPILED_FREE_MAX: Record<FreeLimitKey, number> = { workspaces: 2, teams: 6, "agent.tabs": 35 };

const isKey = (value: string): value is FreeLimitKey => (KEYS as readonly string[]).includes(value);
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
export function reachedLimit(limits: FreeLimits | null, counts: LimitCounts, keys: readonly FreeLimitKey[]): FreeLimitKey | null {
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

/** The limit a daemon refusal names, `fallback` when it predates the key, null for any other error. */
export function refusedLimit(error: unknown, fallback: FreeLimitKey): FreeLimitKey | null {
  const text = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  if (!text.includes(`${FREE_LIMIT_REACHED}:`)) return null;
  const named = /\(limit: ([a-z.]+)\)/.exec(text)?.[1];
  return named && isKey(named) ? named : fallback;
}

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
    case "agent.tabs": return `The free version allows ${count} tabs running at once.`;
  }
}
