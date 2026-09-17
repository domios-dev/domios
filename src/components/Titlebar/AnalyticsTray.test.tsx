import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PluginSnapshotStoreState } from "../../lib/pluginSnapshotStore";

const store: { plugins: Array<{ id: string; capabilities?: { view?: boolean } }> } = { plugins: [] };

const ensurePluginSnapshot = vi.fn(
  (_options?: { force?: boolean; background?: boolean }) => Promise.resolve(),
);

vi.mock("../../lib/pluginSnapshotStore", () => ({
  usePluginSnapshotStore: (): Partial<PluginSnapshotStoreState> => ({
    snapshot: { plugins: store.plugins, channels: [], diff: [] },
  }),
  ensurePluginSnapshot: (options?: { force?: boolean; background?: boolean }) =>
    ensurePluginSnapshot(options),
}));

import { AnalyticsTray } from "./AnalyticsTray";
import { ANALYTICS_PLUGIN_ID, isAnalyticsAvailable } from "./analyticsTrayLogic";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLElement | null = null;

async function render(open: boolean, onToggle = vi.fn()) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<AnalyticsTray open={open} onToggle={onToggle} />));
  return { host, onToggle };
}

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
  store.plugins = [];
});

describe("analytics titlebar entry", () => {
  it("is available only for the installed core plugin that declares its view", () => {
    expect(isAnalyticsAvailable([{ id: ANALYTICS_PLUGIN_ID, capabilities: { view: true } }])).toBe(true);
    expect(isAnalyticsAvailable([{ id: ANALYTICS_PLUGIN_ID, capabilities: { view: false } }])).toBe(false);
    expect(isAnalyticsAvailable([{ id: ANALYTICS_PLUGIN_ID }])).toBe(false);
    expect(isAnalyticsAvailable([{ id: "git", capabilities: { view: true } }])).toBe(false);
  });

  it("renders nothing when the plugin is absent, so it never opens an empty view", async () => {
    const { host } = await render(false);
    expect(host.querySelector('[data-testid="titlebar-analytics"]')).toBeNull();
  });

  it("is a named keyboard button that marks the open view and toggles it", async () => {
    store.plugins = [{ id: ANALYTICS_PLUGIN_ID, capabilities: { view: true } }];
    const { host, onToggle } = await render(true);
    const button = host.querySelector<HTMLButtonElement>('[data-testid="titlebar-analytics"]');
    expect(button?.tagName).toBe("BUTTON");
    expect(button?.getAttribute("type")).toBe("button");
    expect(button?.getAttribute("aria-label")).toBe("Analytics");
    expect(button?.getAttribute("aria-current")).toBe("page");
    expect(button?.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
    await act(async () => button!.click());
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("drops the open marker when the view is closed", async () => {
    store.plugins = [{ id: ANALYTICS_PLUGIN_ID, capabilities: { view: true } }];
    const { host } = await render(false);
    expect(host.querySelector('[data-testid="titlebar-analytics"]')?.hasAttribute("aria-current")).toBe(false);
  });
});
