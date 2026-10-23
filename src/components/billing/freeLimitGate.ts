import { useSyncExternalStore } from "react";
import { localFetch } from "../../lib/webApi";
import { checkoutTheme } from "./checkoutTheme";
import { reachedLimit, refusedLimit, type FreeLimitKey, type FreeLimits, type LimitCounts, type LocalLimitKey, type MeshMachinesLimit } from "./freeLimitModel";

/** The backend checks out commercial keys only, never the bare plan name. */
export const UPGRADE_PLAN_KEY = "pro_monthly";

/** A Mesh prompt carries the server's own count and the refused request it answers. */
export type FreeLimitPrompt =
  | { key: LocalLimitKey }
  | ({ key: "mesh.machines"; requestId: string } & MeshMachinesLimit);

let limits: FreeLimits | null = null;
let readCounts: () => LimitCounts | null = () => null;
let prompt: FreeLimitPrompt | null = null;
const listeners = new Set<() => void>();

function setPrompt(next: FreeLimitPrompt | null) {
  if (prompt === next || (prompt && next && prompt.key === next.key && next.key !== "mesh.machines")) return;
  prompt = next;
  for (const listener of listeners) listener();
}

/** One plans modal for the app: the latest daemon limits, the live counts, and which limit is being shown. */
export const freeLimitGate = {
  setLimits(next: FreeLimits | null) { limits = next; },
  limits: () => limits,
  countWith(read: () => LimitCounts): () => void {
    readCounts = read;
    return () => { if (readCounts === read) readCounts = () => null; };
  },
  open: (key: LocalLimitKey) => setPrompt({ key }),
  openMeshMachines: (requestId: string, limit: MeshMachinesLimit) => setPrompt({ key: "mesh.machines", requestId, ...limit }),
  close: () => setPrompt(null),
  prompt: (): FreeLimitKey | null => prompt?.key ?? null,
  current: () => prompt,
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => { listeners.delete(listener); };
  },
};

export function useFreeLimitPrompt(): FreeLimitPrompt | null {
  return useSyncExternalStore(freeLimitGate.subscribe, freeLimitGate.current, freeLimitGate.current);
}

/** Opens the modal instead of creating when the client already knows a limit is full under enforcement. */
export function precheckFreeLimit(keys: readonly LocalLimitKey[]): boolean {
  const counts = readCounts();
  const reached = counts && reachedLimit(limits, counts, keys);
  if (!reached) return false;
  freeLimitGate.open(reached);
  return true;
}

/** Opens the modal for a daemon free-limit refusal; false for every other error. */
export function raiseFreeLimit(error: unknown, fallback: LocalLimitKey): boolean {
  const key = refusedLimit(error, fallback);
  if (key) freeLimitGate.open(key);
  return key !== null;
}

/** A daemon limit refusal opens the modal and resolves undefined; every other error still rejects. */
export async function openOnRefusal<T>(fallback: LocalLimitKey, create: () => Promise<T>): Promise<T | undefined> {
  try {
    return await create();
  } catch (error) {
    if (raiseFreeLimit(error, fallback)) return undefined;
    throw error;
  }
}

/** `openOnRefusal` behind the pre-check, for creations that always add to `keys`. */
export async function guardFreeLimit<T>(keys: readonly [LocalLimitKey, ...LocalLimitKey[]], create: () => Promise<T>): Promise<T | undefined> {
  return precheckFreeLimit(keys) ? undefined : openOnRefusal(keys[0], create);
}

export async function upgradePlan(planKey: string = UPGRADE_PLAN_KEY): Promise<void> {
  const response = await localFetch("/api/account/upgrade", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ plan: planKey, theme: checkoutTheme() ?? undefined }),
  });
  if (!response.ok) throw new Error(await response.text());
}
