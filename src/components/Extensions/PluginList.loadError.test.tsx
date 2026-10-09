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

it("shows the load error on the failing plugin row only", async () => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const message = "parse: unknown field `prompt`";
  const snapshot: PluginSnapshot = {
    plugins: [
      {
        id: "claude",
        display_name: "Claude",
        version: "1.0.0",
        load_error: { message, path: "/plugins/provider-claude/plugin.json", at_ms: 1, kept_previous: true },
      },
      { id: "healthy", display_name: "Healthy", version: "1.0.0", load_error: null },
    ],
    channels: [],
    diff: [],
    marketplace: [],
  };
  const entries = buildCatalog(snapshot);
  await act(async () => root!.render(<PluginList
    entries={entries} selectedId={null} onSelect={vi.fn()} query="" onQuery={vi.fn()}
    filter="all" onFilter={vi.fn()} updateCount={updateCount(entries)}
    onRemoveChannel={vi.fn()} activities={new Map()} activityDisplayMap={new Map()}
    channels={[]} selectedChannel={null} channelPluginCounts={new Map()} onSelectChannel={vi.fn()}
  />));

  const row = (id: string) => host!.querySelector(`[data-testid="extensions-plugin-row"][data-plugin-id="${id}"]`)!;
  expect(row("claude").getAttribute("data-plugin-load-error")).toBe("true");
  expect(row("claude").querySelector('[data-testid="extensions-plugin-badge-error"]')).not.toBeNull();
  const line = row("claude").querySelector('[role="alert"][data-testid="extensions-plugin-load-error"]');
  expect(line?.textContent).toBe(message);
  expect(line?.getAttribute("title")).toContain("/plugins/provider-claude/plugin.json");
  expect(row("healthy").getAttribute("data-plugin-load-error")).toBe("false");
  expect(row("healthy").querySelector('[data-testid="extensions-plugin-load-error"]')).toBeNull();
});
