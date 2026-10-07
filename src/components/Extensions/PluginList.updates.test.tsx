import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { PluginList } from "./PluginList";
import { buildCatalog, updateCount } from "./pluginCatalog";
import type { PluginSnapshot } from "./types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root | null = null;
let host: HTMLDivElement | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
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
