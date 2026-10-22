import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { PluginList } from "./PluginList";
import { buildCatalog, updateCount } from "./pluginCatalog";
import type { PluginSnapshot } from "./types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const state = vi.hoisted(() => ({ panel: null as null | { snapshot: { errors: Record<string, string> }; diagnostics: { channel: string; message: string }[]; pluginHasUpdate: (id: string) => boolean } }));
vi.mock("../../hooks/useUpdatePanel", () => ({ useOptionalUpdatePanel: () => state.panel }));
let root: Root | null = null;
let host: HTMLDivElement | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  state.panel = null;
  root = null;
  host = null;
});

it("renders badge and Updates count from the installed decision across refreshes", async () => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const onFilter = vi.fn();
  const snapshot: PluginSnapshot = {
    plugins: [{ id: "demo", display_name: "Demo", version: "0.1.0", update: { available: false, reason: "same_version" } }],
    channels: [], diff: [],
    marketplace: [{ id: "demo", channel: "store", drift: "hash_drift", remote_version: "0.1.0", installed: true }],
  };
  const render = async () => {
    const entries = buildCatalog(snapshot);
    await act(async () => root!.render(<PluginList
      entries={entries} selectedId={null} onSelect={vi.fn()} query="" onQuery={vi.fn()}
      filter="all" onFilter={onFilter} updateCount={updateCount(entries)}
      onRemoveChannel={vi.fn()} activities={new Map()} activityDisplayMap={new Map()}
      channels={[]} selectedChannel={null} channelPluginCounts={new Map()} onSelectChannel={vi.fn()}
    />));
  };
  const updatesButton = () => [...host!.querySelectorAll("button")].find((button) => button.textContent?.startsWith("Updates"))!;
  await render();
  expect(host.querySelector('[role="status"][aria-label="Update available"]')).toBeNull();
  expect(updatesButton().textContent).toBe("Updates");
  snapshot.plugins[0].update = { available: true, reason: "newer_version", remote_version: "0.2.0" };
  await render();
  expect(host.querySelectorAll('[role="status"][aria-label="Update available"]')).toHaveLength(1);
  expect(updatesButton().textContent).toBe("Updates 1");
  await act(async () => updatesButton().click());
  expect(onFilter).toHaveBeenCalledExactlyOnceWith("updates");
  snapshot.plugins[0].version = "0.2.0";
  snapshot.plugins[0].update = { available: false, reason: "same_version" };
  await render();
  expect(host.querySelector('[role="status"][aria-label="Update available"]')).toBeNull();
  expect(updatesButton().textContent).toBe("Updates");
});

it("shows one channel diagnostic and failed row indicators without update offers", async () => {
  state.panel = { snapshot: { errors: { store: "opaque channel diagnostic" } }, diagnostics: [{ channel: "store", message: "opaque channel diagnostic" }], pluginHasUpdate: () => false };
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  const entries = buildCatalog({ plugins: ["first", "second"].map(id => ({ id, version: "1.0.0", source: { kind: "channel" as const, channel: "store", path: id }, update: { available: true, remote_version: "2.0.0", reason: "newer_version" } })), channels: [], diff: [] });
  await act(async () => root!.render(<PluginList entries={entries} selectedId={null} onSelect={vi.fn()} query="" onQuery={vi.fn()} filter="all" onFilter={vi.fn()} updateCount={0} onRemoveChannel={vi.fn()} activities={new Map()} activityDisplayMap={new Map()} channels={[]} selectedChannel={null} channelPluginCounts={new Map()} onSelectChannel={vi.fn()} />));
  expect(host.querySelectorAll('section[aria-label="store updates unavailable"]')).toHaveLength(1);
  expect(host.querySelectorAll("pre")).toHaveLength(1);
  expect(host.querySelector("details")?.open).toBe(false);
  expect(host.querySelector('[aria-label="Update available"]')).toBeNull();
  expect(host.querySelector('[data-plugin-id="first"]')?.textContent).toContain("1.0.0 → 2.0.0");
  for (const id of ["first", "second"]) expect(host.querySelector(`[aria-label="${id} update check failed"]`)).not.toBeNull();
});
