import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

const documentState = vi.hoisted(() => ({ html: "<html></html>" as string | undefined, error: null as string | null, media: undefined as undefined | ((id: string) => Promise<unknown>), dispose: undefined as undefined | (() => void) }));
const documentLoad = vi.hoisted(() => vi.fn(() => documentState));
const listeners = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>());

vi.mock("../../lib/bridge", () => ({
  invoke: vi.fn(),
  listen: vi.fn((event: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(event, handler);
    return Promise.resolve(() => listeners.delete(event));
  }),
}));
vi.mock("../../lib/webApi", () => ({
  IS_WEB: false,
  daemonHttpBase: () => "http://127.0.0.1:7686",
  daemonAuthToken: () => "token",
}));
vi.mock("./usePluginDocument", () => ({ usePluginDocument: documentLoad }));
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
  documentState.html = "<html></html>";
  documentState.error = null;
  documentState.media = undefined;
  documentState.dispose = undefined;
  documentLoad.mockClear();
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
  it("registers media only for the exact current iframe and sends no frame-provided path", async () => {
    const media = vi.fn(async () => ({ handle: "opaque", url: "scoped" }));
    documentState.media = media;
    await mount(<PluginView pluginId="fixture" api={undefined} embedded />);
    const iframe = host!.querySelector("iframe")!;
    const postMessage = vi.fn();
    const ownWindow = { postMessage };
    Object.defineProperty(iframe, "contentWindow", { configurable: true, value: ownWindow });
    await act(async () => window.dispatchEvent(new MessageEvent("message", { source: window, data: { __ctMedia: 1, id: "foreign", path: "/secret" } })));
    expect(media).not.toHaveBeenCalled();
    await act(async () => window.dispatchEvent(new MessageEvent("message", { source: ownWindow as unknown as Window, data: { __ctMedia: 2, id: "own", path: "/secret", plugin: "foreign" } })));
    expect(media).toHaveBeenCalledExactlyOnceWith("own");
    expect(postMessage).toHaveBeenCalledWith({ __ctReply: 2, result: { handle: "opaque", url: "scoped" } }, "*");
  });
  it("does not reply to a replaced frame after media registration finishes", async () => {
    let resolve!: (value: unknown) => void;
    documentState.media = () => new Promise((done) => { resolve = done; });
    await mount(<PluginView pluginId="fixture" api={undefined} embedded />);
    const iframe = host!.querySelector("iframe")!;
    const postMessage = vi.fn();
    const ownWindow = { postMessage };
    Object.defineProperty(iframe, "contentWindow", { configurable: true, value: ownWindow });
    await act(async () => window.dispatchEvent(new MessageEvent("message", { source: ownWindow as unknown as Window, data: { __ctMedia: 2, id: "own" } })));
    Object.defineProperty(iframe, "contentWindow", { configurable: true, value: { postMessage: vi.fn() } });
    await act(async () => resolve({ handle: "opaque" }));
    expect(postMessage).not.toHaveBeenCalledWith(expect.objectContaining({ __ctReply: 2 }), "*");
  });
  it("revokes media on an in-frame reload even when the surface instance is unchanged", async () => {
    documentState.dispose = vi.fn();
    await mount(<PluginView pluginId="fixture" api={undefined} embedded />);
    const iframe = host!.querySelector("iframe")!;
    await act(async () => iframe.dispatchEvent(new Event("load")));
    expect(documentState.dispose).not.toHaveBeenCalled();
    await act(async () => iframe.dispatchEvent(new Event("load")));
    expect(documentState.dispose).toHaveBeenCalledTimes(1);
    expect(documentLoad).toHaveBeenLastCalledWith(expect.stringContaining("reload=1"));
  });
  it("keeps a plugin mount exception visible and retries a fresh document", async () => {
    await mount(<PluginView pluginId="fixture" api={undefined} embedded />);
    const iframe = host!.querySelector("iframe")!;
    const ownWindow = iframe.contentWindow;
    await act(async () => window.dispatchEvent(new MessageEvent("message", {
      source: ownWindow, data: { __ctUiState: { status: "error", reason: "Target container is not a DOM element." } },
    })));
    expect(host!.querySelector('[role="alert"]')?.textContent).toContain("Target container is not a DOM element.");
    await act(async () => [...host!.querySelectorAll<HTMLButtonElement>("button")].find(button => button.textContent === "Retry plugin UI")!.click());
    expect(documentLoad).toHaveBeenLastCalledWith(expect.stringContaining("reload=1"));
    expect(host!.querySelector('[role="alert"]')).toBeNull();
  });

  it("does not mount a blank iframe while the bundle is being fetched", async () => {
    documentState.html = undefined;
    await mount(<PluginView pluginId="fixture" api={undefined} embedded />);
    expect(host!.querySelector("iframe")).toBeNull();
    expect(host!.querySelector('[role="status"]')).not.toBeNull();
  });

  it("accepts rendered readiness only from its own frame", async () => {
    await mount(<PluginView pluginId="fixture" api={undefined} />);
    const frame = host!.querySelector("iframe")!;
    await act(async () => window.dispatchEvent(new MessageEvent("message", {
      source: window, data: { __ctUiState: { status: "ready" } },
    })));
    expect(host!.querySelector('[role="status"]')).not.toBeNull();
    await act(async () => window.dispatchEvent(new MessageEvent("message", {
      source: frame.contentWindow, data: { __ctUiState: { status: "ready" } },
    })));
    expect(host!.querySelector('[role="status"]')).toBeNull();
    expect(host!.querySelector('[aria-hidden="false"]')).not.toBeNull();
  });

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

describe("view surface invalidation", () => {
  it("refreshes only the exact plugin frame instance and unsubscribes on close", async () => {
    const surface = { surfaceId: "board", instanceId: "one", tabId: "surface-one" };
    await mount(<PluginView pluginId="studio" api={undefined} surface={surface} />);
    const frame = host!.querySelector("iframe")!;
    const post = vi.spyOn(frame.contentWindow!, "postMessage");
    const emit = listeners.get("surface-state-invalidated")!;
    const own = { plugin: "studio", surface: "board", instance: "one", tab: "surface-one" };
    for (const wrong of [{plugin:"other"},{surface:"other"},{instance:"old"},{tab:"other"}]) {
      await act(async () => emit({payload:{...own,...wrong}}));
    }
    expect(post).not.toHaveBeenCalled();
    await act(async () => emit({payload:own}));
    expect(post).toHaveBeenCalledWith({__ctSurfaceStateChanged:true}, "*");
    await act(async () => root!.unmount());
    root = null;
    expect(listeners.has("surface-state-invalidated")).toBe(false);
    post.mockClear();
    await act(async () => emit({payload:own}));
    expect(post).not.toHaveBeenCalled();
  });

  it("reopened frames ignore the previous instance and catch up on their own readiness", async () => {
    await mount(<PluginView pluginId="studio" api={undefined} surface={{surfaceId:"board",instanceId:"one",tabId:"tab"}} />);
    await act(async () => root!.render(<PluginView pluginId="studio" api={undefined} surface={{surfaceId:"board",instanceId:"two",tabId:"tab"}} />));
    const frame = host!.querySelector("iframe")!;
    const post = vi.spyOn(frame.contentWindow!, "postMessage");
    await act(async () => listeners.get("surface-state-invalidated")!({payload:{plugin:"studio",surface:"board",instance:"one",tab:"tab"}}));
    expect(post).not.toHaveBeenCalled();
    await act(async () => window.dispatchEvent(new MessageEvent("message", {source:window,data:{__ctReady:true}})));
    expect(post).not.toHaveBeenCalled();
    await act(async () => window.dispatchEvent(new MessageEvent("message", {source:frame.contentWindow,data:{__ctReady:true}})));
    expect(post).toHaveBeenCalledWith({__ctSurfaceStateChanged:true}, "*");
  });
});
