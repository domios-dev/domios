import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { AppConfig } from "../../lib/types";

const runtime = vi.hoisted(() => ({ installing: false, status: "idle", downloadedBytes: 0, downloadTotalBytes: null as number | null }));
vi.mock("../../hooks/useUpdate", () => ({ useUpdate: () => ({
  currentVersion: "opaque-version", refresh: vi.fn(), applyUpdate: vi.fn(), ...runtime,
}) }));
import { UpdateSection } from "./UpdateSection";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("defaults automatic provider updates on and persists the opt-out once", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const onUpdate = vi.fn();
  try {
    await act(async () => root.render(<UpdateSection config={{} as AppConfig} onUpdate={onUpdate} />));
    const toggle = host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Update provider CLIs automatically"]');
    expect(toggle?.getAttribute("aria-checked")).toBe("true");
    await act(async () => toggle!.click());
    expect(onUpdate).toHaveBeenCalledExactlyOnceWith("updater", { auto_update_provider_clis: false });
    await act(async () => root.render(<UpdateSection config={{ updater: { auto_update_provider_clis: false } } as AppConfig} onUpdate={onUpdate} />));
    expect(toggle?.getAttribute("aria-checked")).toBe("false");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});


it("reports measured download progress and keeps installation indeterminate", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    Object.assign(runtime, { installing: true, status: "downloading", downloadedBytes: 25, downloadTotalBytes: 100 });
    await act(async () => root.render(<UpdateSection config={{} as AppConfig} onUpdate={vi.fn()} />));
    expect(host.querySelector('progress[aria-label="Domios update progress"]')?.getAttribute("value")).toBe("25");
    Object.assign(runtime, { status: "applying" });
    await act(async () => root.render(<UpdateSection config={{} as AppConfig} onUpdate={vi.fn()} />));
    expect(host.querySelector("progress")?.hasAttribute("value")).toBe(false);
    expect([...host.querySelectorAll("button")].find(button => button.textContent === "Check again")?.disabled).toBe(true);
  } finally { Object.assign(runtime, { installing: false, status: "idle", downloadedBytes: 0, downloadTotalBytes: null }); await act(async () => root.unmount()); host.remove(); }
});
