/**
 * In-app purchase is off for now: plan cards, checkout, upgrades and plan changes give way to the
 * community free-key notice. The billing code and server contracts stay; flip this to bring them back.
 */
export const PURCHASES_ENABLED = false;

/** The free-key action that stands in for a purchase call to action while purchase is off. */
export const FREE_KEY_ACTION = "Get a free key";

/** Names an action that leads to plans: its purchase label when purchase is on, the free-key action when it is off. */
export function plansAction(purchaseLabel: string, freeKeyLabel: string = FREE_KEY_ACTION): string {
  return PURCHASES_ENABLED ? purchaseLabel : freeKeyLabel;
}
