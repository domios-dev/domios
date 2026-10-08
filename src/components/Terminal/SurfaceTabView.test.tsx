import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../lib/bridge", () => ({
  invoke: vi.fn(),
  listen: vi.fn(() => Promise.resolve(() => undefined)),
}));
vi.mock("../../lib/webApi", () => ({
  daemonHttpBase: () => "http://127.0.0.1:7686",
  daemonAuthToken: () => "token",
}));
vi.mock("../../lib/pluginBubbleCache", () => ({ invalidatePluginBubble: vi.fn() }));

import { SurfaceTabView } from "./SurfaceTabView";
import type { SurfaceTabBacking, Tab } from "../../lib/types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// The view's iframe must never reach a live daemon from a unit test.
(window as unknown as { happyDOM: { settings: { disableIframePageLoading: boolean } } }).happyDOM.settings.disableIframePageLoading = true;

let root: Root | null = null;
let host: HTMLElement | null = null;

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
});

async function mount(ui: ReactElement) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(ui));
}

const backing: SurfaceTabBacking = {
  kind: "surface",
  plugin_id: "dash",
  surface_id: "board",
  instance_id: "inst-1",
  owner_label: true,
};
const tab: Tab = { id: "surface-tab", title: "Board", tab_type: "Terminal", pty_id: null, cwd: null, backing, parent_tab_id: "owner" };

function button(name: string): HTMLButtonElement {
  const match = [...host!.querySelectorAll("button")].find((candidate) => candidate.getAttribute("aria-label") === name);
  if (!match) throw new Error(`no button named ${name}`);
  return match;
}

describe("surface tab view", () => {
  it("hosts the plugin view for this surface tab", async () => {
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" onClose={vi.fn()} isVisible />);
    const frame = host!.querySelector("iframe");
    expect(frame?.getAttribute("data-plugin-id")).toBe("dash");
    const query = new URL(frame!.getAttribute("src")!).searchParams;
    expect(query.get("surface")).toBe("board");
    expect(query.get("instance")).toBe("inst-1");
    expect(query.get("tab")).toBe("surface-tab");
    expect(host!.querySelector("section")?.getAttribute("aria-label")).toBe("Jev's Board");
  });

  it("fires the close handler once from the header close button", async () => {
    const onClose = vi.fn();
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" onClose={onClose} isVisible />);
    await act(async () => button("Close Jev's Board").click());
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("focuses the owner tab from the owner link", async () => {
    const onFocusOwner = vi.fn();
    await mount(
      <SurfaceTabView tab={tab} backing={backing} ownerName="Jev" onFocusOwner={onFocusOwner} onClose={vi.fn()} isVisible />,
    );
    await act(async () => button("Open owner tab Jev").click());
    expect(onFocusOwner).toHaveBeenCalledTimes(1);
  });

  it("offers no owner link when the owner is gone", async () => {
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName={null} onFocusOwner={vi.fn()} onClose={vi.fn()} isVisible />);
    const names = [...host!.querySelectorAll("button")].map((candidate) => candidate.getAttribute("aria-label"));
    expect(names).toEqual(["Close Board"]);
  });
});
