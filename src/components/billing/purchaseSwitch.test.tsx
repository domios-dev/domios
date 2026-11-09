import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { useAccount } from "../Settings/useAccount";
import { applyAccountPurchaseSwitch, plansAction, purchasesEnabled, usePurchasesEnabled, FREE_KEY_ACTION } from "./purchaseSwitch";

const transport = vi.hoisted(() => ({ status: {} as Record<string, unknown> }));
vi.mock("../../lib/webApi", () => ({
  localFetch: vi.fn(async () => ({ ok: true, json: async () => transport.status })),
}));
vi.mock("../../lib/bridge", () => ({ listen: vi.fn(async () => () => {}) }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => applyAccountPurchaseSwitch({}));

it("only an explicit true in account status turns purchase on", () => {
  for (const value of [undefined, false, "true", 1, null]) {
    applyAccountPurchaseSwitch({ purchases_enabled: value });
    expect(purchasesEnabled()).toBe(false);
  }
  applyAccountPurchaseSwitch({ purchases_enabled: true });
  expect(purchasesEnabled()).toBe(true);
});

it("account status drives the purchase switch, its hook and plan actions", async () => {
  function Harness() {
    useAccount();
    return <output aria-label="Purchases">{usePurchasesEnabled() ? "on" : "off"}</output>;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  const shown = () => host.querySelector('output[aria-label="Purchases"]')?.textContent;
  try {
    transport.status = { identity: null, pending: false, purchases_enabled: false };
    await act(async () => root.render(<Harness />));
    expect(shown()).toBe("off");
    expect(plansAction("See plans")).toBe(FREE_KEY_ACTION);
    transport.status = { identity: null, pending: false, purchases_enabled: true };
    await act(async () => { window.dispatchEvent(new Event("domios:account-changed")); });
    expect(shown()).toBe("on");
    expect(plansAction("See plans")).toBe("See plans");
  } finally {
    await act(async () => root.unmount());
  }
});
