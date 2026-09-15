import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

const listeners = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>());

vi.mock("../../lib/bridge", () => ({
  invoke: vi.fn(),
  listen: vi.fn((event: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(event, handler);
    return Promise.resolve(() => listeners.delete(event));
  }),
}));
vi.mock("../../lib/webApi", () => ({
  daemonHttpBase: () => "http://127.0.0.1:7686",
  daemonAuthToken: () => "token",
}));
vi.mock("../../lib/pluginBubbleCache", () => ({ invalidatePluginBubble: vi.fn() }));

import { KPI_CHANGE_NOTICE, KPI_RECORDED_EVENT } from "../../lib/kpiChanges";
import { PLUGIN_THEME_VARS, PluginView } from "./PluginView";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLElement | null = null;

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
  listeners.clear();
});

function pressEscape() {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
}

async function mount(ui: ReactElement) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(ui));
}

describe("plugin view activation", () => {
  it("forwards the theme tokens shared components read, not only the base palette", () => {
    for (const token of ["--ct-bg", "--ct-surface", "--ct-border", "--ct-muted-fg", "--ct-magenta", "--ct-cyan", "--ct-green", "--ct-hover"]) {
      expect(PLUGIN_THEME_VARS).toContain(token);
    }
  });

  it("closes on Escape only while the kept-mounted view is shown", async () => {
    const onClose = vi.fn();
    await mount(<PluginView pluginId="analytics" api={undefined} active={false} onClose={onClose} />);
    pressEscape();
    expect(onClose).not.toHaveBeenCalled();

    await act(async () => root!.render(<PluginView pluginId="analytics" api={undefined} active onClose={onClose} />));
    pressEscape();
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("posts a KPI change notice to an opted-in view only while it is shown", async () => {
    await mount(<PluginView pluginId="analytics" api={undefined} active={false} kpiNotices />);
    const iframe = host!.querySelector("iframe")!;
    const postMessage = vi.fn();
    Object.defineProperty(iframe, "contentWindow", { configurable: true, value: { postMessage } });
    const notice = { [KPI_CHANGE_NOTICE]: true };

    listeners.get(KPI_RECORDED_EVENT)!({ payload: { plan_ids: [] } });
    expect(postMessage).not.toHaveBeenCalledWith(notice, "*");

    await act(async () => root!.render(<PluginView pluginId="analytics" api={undefined} active kpiNotices />));
    expect(postMessage).toHaveBeenCalledWith({ __ctActive: true }, "*");
    postMessage.mockClear();
    listeners.get(KPI_RECORDED_EVENT)!({ payload: { plan_ids: ["pl_1"] } });
    expect(postMessage).toHaveBeenCalledTimes(1);
    expect(postMessage).toHaveBeenCalledWith(notice, "*");
  });

  it("does not subscribe a view that has not opted into KPI notices", async () => {
    await mount(<PluginView pluginId="git" api={undefined} active />);
    expect(listeners.has(KPI_RECORDED_EVENT)).toBe(false);
  });
});
