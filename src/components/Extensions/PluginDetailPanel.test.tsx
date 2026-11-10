import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { PluginDetailPanel } from "./PluginDetailPanel";
import type { CatalogEntry } from "./pluginCatalog";
import { setPluginPermission } from "../../lib/pluginConsent";

vi.mock("../../lib/pluginConsent", () => ({ setPluginPermission: vi.fn() }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const entry: CatalogEntry = {
  id: "managed-capability", displayName: "Managed capability", installed: true,
  inMarketplace: true, system: true, verified: true, removable: false,
  dev: false, compatible: true, updateAvailable: false,
};

it.each([
  { system: true },
  { system: false, dev: true },
  { system: false, installed: false, compatible: false, blockedReason: "opaque host requirement" },
])("renders a disabled action explanation as information: %j", async (patch) => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = async (value: CatalogEntry) => {
    await act(async () => root.render(<PluginDetailPanel entry={value} config={null} busy={null}
      onAction={() => {}} onHostGrant={() => {}} onSave={async () => {}}
      onUpdateFullConfig={() => {}} onGone={() => {}} activityDisplayMap={new Map()} />));
  };
  try {
    const value = { ...entry, ...patch };
    await render(value);
    const note = host.querySelector('[role="note"]');
    expect(note).not.toBeNull();
    expect(note?.querySelector("details")).toBeNull();
    expect(host.querySelector('[role="status"], [role="alert"]')).toBeNull();
    await render({ ...value, loadError: { message: "opaque load failure", path: "plugin.js", atMs: 1, keptPrevious: false } });
    const error = host.querySelector('[role="status"]');
    expect(error).not.toBeNull();
    expect(error?.querySelector("details pre")?.textContent).toContain("opaque load failure");
    expect(host.querySelector('[role="note"]')).not.toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("retains an accessible error when a permission operation fails", async () => {
  vi.mocked(setPluginPermission).mockRejectedValueOnce(new Error("opaque operation failure"));
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<PluginDetailPanel
      entry={{ ...entry, sensitivePermissions: [{ key: "local_recording", label: "Record input", allowed: false, decided: true }] }}
      config={null} busy={null} onAction={() => {}} onHostGrant={() => {}} onSave={async () => {}}
      onUpdateFullConfig={() => {}} onGone={() => {}} activityDisplayMap={new Map()} />));
    const toggle = host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Record input"]');
    expect(toggle).not.toBeNull();
    await act(async () => toggle!.click());
    expect(setPluginPermission).toHaveBeenCalledExactlyOnceWith(entry.id, "local_recording", true);
    expect(host.querySelector('[role="alert"] details pre')?.textContent).toContain("opaque operation failure");
    expect(host.querySelector('[role="note"]')).not.toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("opens a plugin interface through the host surface navigation", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const open = vi.fn();
  window.addEventListener("codeterm:open-panel", open);
  const render = async (patch: Partial<CatalogEntry>) => {
    await act(async () => root.render(<PluginDetailPanel
      entry={{ ...entry, capabilities: { view: true }, ...patch }} config={null} busy={null}
      onAction={() => {}} onHostGrant={() => {}} onSave={async () => {}}
      onUpdateFullConfig={() => {}} onGone={() => {}} activityDisplayMap={new Map()} />));
  };
  const interfaceButton = () => Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "Open interface");
  try {
    await render({});
    expect(interfaceButton()).toBeDefined();
    await act(async () => interfaceButton()!.click());
    expect(open).toHaveBeenCalledTimes(1);
    expect((open.mock.calls[0][0] as CustomEvent).detail).toEqual({ panel: `view:${entry.id}` });
    for (const patch of [{ installed: false }, { pendingReload: true }, { capabilities: {} }]) {
      await render(patch);
      expect(interfaceButton()).toBeUndefined();
    }
  } finally {
    window.removeEventListener("codeterm:open-panel", open);
    await act(async () => root.unmount());
    host.remove();
  }
});
