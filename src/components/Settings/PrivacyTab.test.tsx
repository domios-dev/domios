import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { PRODUCT_EVENT_CATALOG } from "../../lib/productAnalytics.generated";

const localJson = vi.hoisted(() => vi.fn().mockResolvedValue({ consent: "off", queued: 0, has_device_id: false, has_user_id: false }));
vi.mock("../../lib/webApi", () => ({ localJson }));
import { PrivacyTab } from "./PrivacyTab";

it("the collection link opens the catalog without emitting analytics", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<PrivacyTab />));
    const link = [...host.querySelectorAll("a")].find(element => element.textContent === "What we collect");
    expect(link).toBeDefined();
    expect(host.querySelector('[aria-label="Analytics collection catalog"]')).toBeNull();
    await act(async () => link!.click());
    const catalog = host.querySelector('[aria-label="Analytics collection catalog"]');
    expect(catalog).not.toBeNull();
    expect([...catalog!.querySelectorAll("code")].map(element => element.textContent)).toEqual(Object.keys(PRODUCT_EVENT_CATALOG));
    expect(localJson.mock.calls).toEqual([["GET", "/api/analytics/status"]]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
