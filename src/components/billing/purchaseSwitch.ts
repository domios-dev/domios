import { useSyncExternalStore } from "react";

/**
 * In-app purchase follows the remote purchase switch the account status reports. Until it says
 * so, plan cards, checkout, upgrades and plan changes give way to the community free-key notice.
 */
export const PURCHASES_DEFAULT = false;

let enabled = PURCHASES_DEFAULT;
const listeners = new Set<() => void>();

export function purchasesEnabled(): boolean {
  return enabled;
}

/** Applies the account status value; only an explicit `true` turns purchase on. */
export function applyAccountPurchaseSwitch(status: { purchases_enabled?: unknown }): void {
  const next = status.purchases_enabled === true;
  if (next === enabled) return;
  enabled = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function usePurchasesEnabled(): boolean {
  return useSyncExternalStore(subscribe, purchasesEnabled, () => PURCHASES_DEFAULT);
}

/** The free-key action that stands in for a purchase call to action while purchase is off. */
export const FREE_KEY_ACTION = "Get a free key";

/** Names an action that leads to plans: its purchase label when purchase is on, the free-key action when it is off. */
export function plansAction(purchaseLabel: string, freeKeyLabel: string = FREE_KEY_ACTION): string {
  return purchasesEnabled() ? purchaseLabel : freeKeyLabel;
}
