import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
const localJson = vi.hoisted(() => vi.fn());
vi.mock("../../lib/webApi", async original => ({ ...await original<typeof import("../../lib/webApi")>(), localJson }));
import { AnalyticsToggle } from "./AnalyticsToggle";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let close: (() => Promise<void>) | undefined;
afterEach(async () => { await close?.(); localJson.mockReset(); });
const status = (consent: "on" | "off") => ({ consent, queued: 0, has_device_id: false, has_user_id: false });
async function mount() {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  close = async () => { await act(async () => root.unmount()); host.remove(); };
  await act(async () => root.render(<AnalyticsToggle />));
  const toggle = host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Share product analytics"]')!;
  return { host, toggle };
}
it("shows the default ON and synchronizes opt-out and explicit opt-in", async () => {
  localJson.mockResolvedValueOnce(status("on")).mockResolvedValueOnce(status("off")).mockResolvedValueOnce(status("on"));
  const { host, toggle } = await mount();
  expect(toggle.getAttribute("aria-checked")).toBe("true");
  await act(async () => toggle.click());
  expect(toggle.getAttribute("aria-checked")).toBe("false");
  await act(async () => toggle.click());
  expect(toggle.getAttribute("aria-checked")).toBe("true");
  expect(localJson.mock.calls).toEqual([["GET", "/api/analytics/status"], ["POST", "/api/analytics/consent", { consent: "off" }], ["POST", "/api/analytics/consent", { consent: "on" }]]);
  expect(host.querySelectorAll('[role="switch"]')).toHaveLength(1);
  expect(host.querySelector('[role="dialog"], [role="alert"], [role="status"], a')).toBeNull();
});
it("keeps offline withdrawal off and reports no prompt or toast", async () => {
  localJson.mockResolvedValueOnce(status("on")).mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce(status("off"));
  const { host, toggle } = await mount();
  await act(async () => toggle.click());
  expect(toggle.getAttribute("aria-checked")).toBe("false");
  expect(host.querySelector('[role="dialog"], [role="alert"], [role="status"]')).toBeNull();
});
it("ignores an old ON response after a newer OFF", async () => {
  let finish!: (value: ReturnType<typeof status>) => void;
  const old = new Promise<ReturnType<typeof status>>(resolve => { finish = resolve; });
  localJson.mockResolvedValueOnce(status("off")).mockReturnValueOnce(old).mockResolvedValueOnce(status("off"));
  const { toggle } = await mount();
  await act(async () => toggle.click());
  await act(async () => toggle.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  await act(async () => finish(status("on")));
  expect(toggle.getAttribute("aria-checked")).toBe("false");
});
it("keeps an unavailable background read quiet", async () => {
  localJson.mockRejectedValue(new Error("unavailable"));
  const { host, toggle } = await mount();
  expect(toggle.closest("fieldset")?.disabled).toBe(true);
  expect(host.querySelector('[role="dialog"], [role="alert"], [role="status"]')).toBeNull();
});
