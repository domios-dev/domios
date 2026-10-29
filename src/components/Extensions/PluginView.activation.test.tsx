import { PluginViewChannel } from "../../lib/pluginViewChannel";
import { TestViewPort } from "../../lib/pluginViewChannel.testSupport";
import { act, StrictMode, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const documentState = vi.hoisted(() => ({ channel: undefined as PluginViewChannel | undefined, html: "<html></html>" as string | undefined, error: null as string | null, media: undefined as undefined | ((id: string) => Promise<unknown>), dispose: undefined as undefined | (() => void), invoke: undefined as undefined | ((method: string, args: unknown) => Promise<unknown>) }));
const documentLoad = vi.hoisted(() => vi.fn(() => documentState));
const listeners = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>());

vi.mock("../../lib/pluginConsent", () => ({
  invokeViewWithConsent: async (_plugin: string, operation: () => Promise<unknown>, current: () => boolean) => {
    if (!current()) throw new Error("stale");
    return operation();
  },
}));
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
const hostDeps = vi.hoisted(() => ({
  call: vi.fn(async (_request: Record<string, unknown>): Promise<unknown> => ({ ok: true })),
  writeText: vi.fn(async (_text: string) => undefined),
  download: vi.fn(async () => ({ ok: true as const })),
}));
vi.mock("../../lib/pluginViewHostDeps", () => ({ viewHostDeps: () => hostDeps }));
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
  documentState.channel?.dispose();
  listeners.clear();
  documentState.html = "<html></html>";
  documentState.error = null;
  documentState.media = undefined;
  documentState.dispose = undefined;
  documentState.invoke = undefined;
  documentLoad.mockClear();
});

/** Focus moves into a frame when the user clicks inside it. */
function focusFrame(iframe: HTMLIFrameElement) {
  iframe.tabIndex = 0;
  iframe.focus();
}

function pressEscape() {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
}

async function mount(ui: ReactElement) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(ui));
}

let port: TestViewPort;
beforeEach(() => {
  documentState.channel = new PluginViewChannel(crypto.randomUUID().replaceAll("-", ""));
  port = new TestViewPort();
  hostDeps.call.mockReset().mockResolvedValue({ok:true});
  hostDeps.writeText.mockClear();
});
async function connect() {
  const frame = host!.querySelector("iframe")!;
  await act(async () => { documentState.channel!.accept(frame.contentWindow, port.port); port.prove(); });
  return frame;
}
async function receive(data: unknown) { await act(async () => port.receive(data)); }
const replies = () => port.sent.filter(message => message.__ctReply != null);

it("admits media only over the proven document port, never a window message or frame path", async () => {
  documentState.media = vi.fn(async () => ({handle:"opaque",url:"scoped"}));
  await mount(<PluginView pluginId="fixture" api={undefined} embedded />);
  const frame = await connect();
  await act(async () => window.dispatchEvent(new MessageEvent("message", {source:frame.contentWindow,data:{__ctMedia:1,id:"foreign",path:"/secret"}})));
  expect(documentState.media).not.toHaveBeenCalled();
  await receive({__ctMedia:2,id:"own",path:"/secret",plugin:"foreign"});
  expect(documentState.media).toHaveBeenCalledExactlyOnceWith("own");
  expect(replies()).toEqual([{__ctReply:2,result:{handle:"opaque",url:"scoped"}}]);
});

it("reports mount errors and retries a fresh loader document", async () => {
  await mount(<PluginView pluginId="fixture" api={undefined} embedded />);
  await connect();
  await receive({__ctUiState:{status:"error",reason:"opaque mount failure"}});
  expect(host!.querySelector('[role="alert"]')?.textContent).toContain("opaque mount failure");
  await act(async () => [...host!.querySelectorAll<HTMLButtonElement>("button")].find(button => button.textContent === "Retry plugin UI")!.click());
  expect(documentLoad).toHaveBeenLastCalledWith(expect.stringContaining("reload=1"));
});

it("does not mount a blank iframe during document loading", async () => {
  documentState.html = undefined;
  await mount(<PluginView pluginId="fixture" api={undefined} embedded />);
  expect(host!.querySelector("iframe")).toBeNull();
  expect(host!.querySelector('[role="status"]')).not.toBeNull();
});

it("accepts rendered readiness and shares the complete theme only through the document port", async () => {
  await mount(<PluginView pluginId="fixture" api={undefined} />);
  const frame = await connect();
  await act(async () => window.dispatchEvent(new MessageEvent("message",{source:frame.contentWindow,data:{__ctUiState:{status:"ready"}}})));
  expect(host!.querySelector('[role="status"]')).not.toBeNull();
  await receive({__ctUiState:{status:"ready"}});
  expect(host!.querySelector('[role="status"]')).toBeNull();
  for (const token of ["--ct-bg","--ct-surface","--ct-border","--ct-muted-fg","--ct-magenta","--ct-cyan","--ct-green","--ct-hover"]) expect(PLUGIN_THEME_VARS).toContain(token);
  expect(port.sent.some(message => message.__ctTheme != null)).toBe(true);
  expect(port.sent).toContainEqual({__ctHostSupport:{copyText:true,saveAs:true,reveal:false}});
});

it("closes on Escape only while shown", async () => {
  const close = vi.fn();
  await mount(<PluginView pluginId="analytics" api={undefined} active={false} onClose={close} />);
  pressEscape();
  expect(close).not.toHaveBeenCalled();
  await act(async () => root!.render(<PluginView pluginId="analytics" api={undefined} active onClose={close} />));
  pressEscape();
  expect(close).toHaveBeenCalledTimes(1);
});

it("forwards KPI notices only to an opted-in visible document", async () => {
  await mount(<PluginView pluginId="analytics" api={undefined} active={false} kpiNotices />);
  await connect();
  port.sent = [];
  listeners.get(KPI_RECORDED_EVENT)!({payload:{}});
  expect(port.sent).toEqual([]);
  await act(async () => root!.render(<PluginView pluginId="analytics" api={undefined} active kpiNotices />));
  expect(port.sent).toContainEqual({__ctActive:true});
  port.sent = [];
  listeners.get(KPI_RECORDED_EVENT)!({payload:{}});
  expect(port.sent).toEqual([{[KPI_CHANGE_NOTICE]:true}]);
});

it("does not subscribe a view without KPI opt-in", async () => {
  await mount(<PluginView pluginId="git" api={undefined} />);
  expect(listeners.has(KPI_RECORDED_EVENT)).toBe(false);
});

it("uses app-observed focus and activation rather than host-action args", async () => {
  await mount(<PluginView pluginId="studio" api={undefined} embedded />);
  const frame = await connect();
  focusFrame(frame);
  await receive({__ctHost:1,action:"copyText",args:{text:"fox"},gesture:true,pluginId:"other"});
  expect(replies()).toContainEqual({__ctReply:1,result:{ok:false,code:"denied",reason:"gesture"}});
  Object.defineProperty(navigator,"userActivation",{configurable:true,value:{isActive:true}});
  try {
    const chrome = document.createElement("button");document.body.append(chrome);chrome.focus();
    await receive({__ctHost:2,action:"copyText",args:{text:"fox"}});
    expect(hostDeps.call).not.toHaveBeenCalled();chrome.remove();focusFrame(frame);
    await receive({__ctHost:3,action:"copyText",args:{text:"fox"}});
  } finally { delete (navigator as {userActivation?:unknown}).userActivation; }
  expect(hostDeps.call).toHaveBeenCalledExactlyOnceWith({pluginId:"studio",action:"copyText"});
  expect(hostDeps.writeText).toHaveBeenCalledWith("fox");
  expect(replies()).toContainEqual({__ctReply:3,result:{ok:true}});
  expect(host!.querySelector('[data-testid="plugin-view-copied"]')?.textContent).toBe("Copied from studio");
});

it.each(["hide", "load", "replace"] as const)("ends pending gesture effects after %s", async (transition) => {
  await mount(<PluginView pluginId="studio" api={undefined} embedded />);
  const frame = await connect();focusFrame(frame);
  let finish!: (value:unknown) => void;
  hostDeps.call.mockImplementationOnce(() => new Promise(resolve => {finish=resolve;}));
  Object.defineProperty(navigator,"userActivation",{configurable:true,value:{isActive:true}});
  try { await receive({__ctHost:6,action:"copyText",args:{text:"late"}}); }
  finally { delete (navigator as {userActivation?:unknown}).userActivation; }
  if (transition === "load") await act(async () => frame.dispatchEvent(new Event("load")));
  else if (transition === "hide") {
    await act(async () => root!.render(<PluginView pluginId="studio" api={undefined} embedded active={false} />));
    await act(async () => root!.render(<PluginView pluginId="studio" api={undefined} embedded active />));
  } else await act(async () => root!.render(<PluginView pluginId="other" api={undefined} embedded />));
  await act(async () => finish({ok:true}));
  expect(hostDeps.writeText).not.toHaveBeenCalled();
  expect(replies().some(message => message.__ctReply===6)).toBe(false);
});

it("invalidates only the exact surface and unsubscribes on close", async () => {
  const surface={surfaceId:"board",instanceId:"one",tabId:"tab"};
  await mount(<PluginView pluginId="studio" api={undefined} surface={surface} />);
  await connect();port.sent=[];
  const emit=listeners.get("surface-state-invalidated")!;
  const own={plugin:"studio",surface:"board",instance:"one",tab:"tab"};
  for (const wrong of [{plugin:"other"},{surface:"other"},{instance:"old"},{tab:"other"}]) await act(async () => emit({payload:{...own,...wrong}}));
  expect(port.sent).toEqual([]);
  await act(async () => emit({payload:own}));
  expect(port.sent).toEqual([{__ctSurfaceStateChanged:true}]);
  await act(async () => root!.unmount());root=null;port.sent=[];
  await act(async () => emit({payload:own}));
  expect(port.sent).toEqual([]);
});

it("keeps an early invocation result private while a first observed load is unproven", async () => {
  let finish!: (value:unknown) => void;
  documentState.invoke=vi.fn(() => new Promise(resolve => {finish=resolve;}));
  await mount(<PluginView pluginId="studio" api={undefined} />);
  const frame=await connect();
  await receive({__ctInvoke:1,method:"read",args:{owner:"forged"}});
  expect(documentState.invoke).toHaveBeenCalledWith("read",{owner:"forged"});
  await act(async () => frame.dispatchEvent(new Event("load")));
  await act(async () => finish({private:true}));
  expect(replies()).toEqual([]);
  // Only the original document's private endpoint can prove the next load.
  await act(async () => port.prove());
  expect(replies()).toEqual([{__ctReply:1,error:expect.any(String)}]);
});

it("preserves current document calls across StrictMode effect replay", async () => {
  documentState.invoke=vi.fn(async () => ({current:true}));
  await mount(<StrictMode><PluginView pluginId="studio" api={undefined}/></StrictMode>);
  await connect();
  await receive({__ctInvoke:7,method:"read",args:{}});
  expect(replies()).toEqual([{__ctReply:7,result:{current:true}}]);
});

it("reopened views use a fresh document port and catch up without accepting old instance hints", async () => {
  await mount(<PluginView pluginId="studio" api={undefined} surface={{surfaceId:"board",instanceId:"one",tabId:"tab"}} />);
  await connect();
  const old=port;documentState.channel!.dispose();
  documentState.channel=new PluginViewChannel(crypto.randomUUID().replaceAll("-", ""));
  port=new TestViewPort();
  await act(async () => root!.render(<PluginView pluginId="studio" api={undefined} surface={{surfaceId:"board",instanceId:"two",tabId:"tab"}} />));
  await connect();expect(old.closed).toBe(true);
  expect(port.sent).toContainEqual({__ctSurfaceStateChanged:true});port.sent=[];
  await act(async () => listeners.get("surface-state-invalidated")!({payload:{plugin:"studio",surface:"board",instance:"one",tab:"tab"}}));
  expect(port.sent).toEqual([]);
});
