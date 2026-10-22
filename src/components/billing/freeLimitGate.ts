import { useSyncExternalStore } from "react";
import { localFetch } from "../../lib/webApi";
import { checkoutTheme } from "./checkoutTheme";
import { reachedLimit, refusedLimit, type FreeLimitKey, type FreeLimits, type LimitCounts } from "./freeLimitModel";

/** The backend checks out commercial keys only, never the bare plan name. */
export const UPGRADE_PLAN_KEY = "pro_monthly";

let limits: FreeLimits | null = null;
let readCounts: () => LimitCounts | null = () => null;
let prompt: FreeLimitKey | null = null;
const listeners = new Set<() => void>();

function setPrompt(next: FreeLimitKey | null) {
  if (prompt === next) return;
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
  open: (key: FreeLimitKey) => setPrompt(key),
  close: () => setPrompt(null),
  prompt: () => prompt,
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => { listeners.delete(listener); };
  },
};

export function useFreeLimitPrompt(): FreeLimitKey | null {
  return useSyncExternalStore(freeLimitGate.subscribe, freeLimitGate.prompt, freeLimitGate.prompt);
}

/** Opens the modal instead of creating when the client already knows a limit is full under enforcement. */
export function precheckFreeLimit(keys: readonly FreeLimitKey[]): boolean {
  const counts = readCounts();
  const reached = counts && reachedLimit(limits, counts, keys);
  if (!reached) return false;
  freeLimitGate.open(reached);
  return true;
}

/** Opens the modal for a daemon free-limit refusal; false for every other error. */
export function raiseFreeLimit(error: unknown, fallback: FreeLimitKey): boolean {
  const key = refusedLimit(error, fallback);
  if (key) freeLimitGate.open(key);
  return key !== null;
}

/** A daemon limit refusal opens the modal and resolves undefined; every other error still rejects. */
export async function openOnRefusal<T>(fallback: FreeLimitKey, create: () => Promise<T>): Promise<T | undefined> {
  try {
    return await create();
  } catch (error) {
    if (raiseFreeLimit(error, fallback)) return undefined;
    throw error;
  }
}

/** `openOnRefusal` behind the pre-check, for creations that always add to `keys`. */
export async function guardFreeLimit<T>(keys: readonly [FreeLimitKey, ...FreeLimitKey[]], create: () => Promise<T>): Promise<T | undefined> {
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
