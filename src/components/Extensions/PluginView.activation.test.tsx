import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../lib/bridge", () => ({
  invoke: vi.fn(),
  listen: vi.fn(() => Promise.resolve(() => {})),
}));
vi.mock("../../lib/webApi", () => ({
  daemonHttpBase: () => "http://127.0.0.1:7686",
  daemonAuthToken: () => "token",
}));
vi.mock("../../lib/pluginBubbleCache", () => ({ invalidatePluginBubble: vi.fn() }));

import { PLUGIN_THEME_VARS, PluginView } from "./PluginView";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLElement | null = null;

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
});

function pressEscape() {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
}

describe("plugin view activation", () => {
  it("forwards the theme tokens shared components read, not only the base palette", () => {
    for (const token of ["--ct-bg", "--ct-surface", "--ct-border", "--ct-muted-fg", "--ct-magenta", "--ct-cyan", "--ct-green", "--ct-hover"]) {
      expect(PLUGIN_THEME_VARS).toContain(token);
    }
  });

  it("closes on Escape only while the kept-mounted view is shown", async () => {
    const onClose = vi.fn();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root!.render(<PluginView pluginId="analytics" api={undefined} active={false} onClose={onClose} />));
    pressEscape();
    expect(onClose).not.toHaveBeenCalled();

    await act(async () => root!.render(<PluginView pluginId="analytics" api={undefined} active onClose={onClose} />));
    pressEscape();
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
