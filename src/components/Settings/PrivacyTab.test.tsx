import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { PRODUCT_EVENT_CATALOG } from "../../lib/productAnalytics.generated";

const localJson = vi.hoisted(() => vi.fn().mockResolvedValue({ consent: "off", queued: 0, has_device_id: false, has_user_id: false }));
vi.mock("../../lib/webApi", async (importOriginal) => ({ ...await importOriginal<typeof import("../../lib/webApi")>(), localJson }));
import { PrivacyTab } from "./PrivacyTab";

it("the collection link opens the catalog without emitting analytics", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<PrivacyTab />));
    const trigger = host.querySelector<HTMLButtonElement>('button[aria-controls="analytics-collection"]');
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelector('[aria-label="Analytics collection catalog"]')).toBeNull();
    await act(async () => trigger!.click());
    expect(trigger!.getAttribute("aria-expanded")).toBe("true");
    const catalog = host.querySelector('[aria-label="Analytics collection catalog"]');
    expect(catalog).not.toBeNull();
    expect([...catalog!.querySelectorAll("code")].map(element => element.textContent)).toEqual(Object.keys(PRODUCT_EVENT_CATALOG));
    expect(localJson.mock.calls).toEqual([["GET", "/api/analytics/status"]]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("keeps a failed background status read quiet and announces a failed consent action once", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  try {
    localJson.mockReset().mockRejectedValueOnce(new Error("opaque status read failure"));
    await act(async () => root.render(<PrivacyTab />));
    expect(host.querySelector('[role="status"]')).not.toBeNull();
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.querySelector('section[role="status"][aria-label]')).toBeNull();
    await act(async () => root.render(null));

    localJson.mockReset()
      .mockResolvedValueOnce({ consent: "off", queued: 0, has_device_id: false, has_user_id: false })
      .mockRejectedValueOnce(new Error("opaque consent write failure"));
    await act(async () => root.render(<PrivacyTab />));
    const toggle = host.querySelector('[role="switch"]') as HTMLButtonElement | null;
    expect(toggle).not.toBeNull();
    await act(async () => toggle!.click());
    expect(host.querySelectorAll('section[role="status"][aria-label]')).toHaveLength(1);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
