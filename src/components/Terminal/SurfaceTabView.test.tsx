import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../lib/bridge", () => ({
  invoke: vi.fn(),
  listen: vi.fn(() => Promise.resolve(() => undefined)),
}));
vi.mock("../../lib/webApi", () => ({
  relayTarget: vi.fn(() => null),
  IS_WEB: false,
  GATEWAY_STATE_EVENT: "codeterm-gateway-state",
  daemonHttpBase: () => "http://127.0.0.1:7686",
  daemonAuthToken: () => "token",
}));
const pluginDocument = vi.hoisted(() => vi.fn((_path: string) => ({ html: "<html></html>", error: null })));
vi.mock("../Extensions/usePluginDocument", () => ({ usePluginDocument: pluginDocument }));
vi.mock("../../lib/pluginBubbleCache", () => ({ invalidatePluginBubble: vi.fn() }));

import { relayTarget } from "../../lib/webApi";
import { invoke } from "../../lib/bridge";
import { openInSystemBrowser, SurfaceTabView, type SurfaceTransportFactory } from "./SurfaceTabView";
import { recordSurfaceActivity, SURFACE_PULSE_MS, surfaceActivityStore } from "../../lib/surfaceActivity";
import { surfaceControlStore } from "../../lib/surfaceControlStore";
import type { SurfaceDeclLookup } from "../Surface/surfaceRenderPlan";
import type { SurfaceStreamTransport } from "../Surface/surfaceStreamTransport";
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
  vi.mocked(relayTarget).mockReturnValue(null);
  pluginDocument.mockClear();
  surfaceActivityStore.reset();
  surfaceControlStore.reset();
  vi.useRealTimers();
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

const viewDecl: SurfaceDeclLookup = { status: "ready", declared: { pluginId: "dash", surfaceId: "board", renderer: "view", runtime: null, chrome: undefined } };
const streamDecl: SurfaceDeclLookup = { status: "ready", declared: { pluginId: "dash", surfaceId: "board", renderer: "stream", runtime: "browser", chrome: undefined } };

function fakeTransports() {
  const created: Parameters<SurfaceTransportFactory>[0][] = [];
  const transport: SurfaceStreamTransport = { connect: () => () => undefined, invoke: async <T,>() => undefined as T };
  const factory: SurfaceTransportFactory = (options) => {
    created.push(options);
    return transport;
  };
  return { created, factory };
}

function byRole(role: string, name: string): HTMLElement | null {
  return [...host!.querySelectorAll<HTMLElement>(`[role='${role}']`)].find((element) => element.getAttribute("aria-label") === name) ?? null;
}

function button(name: string): HTMLButtonElement {
  const match = [...host!.querySelectorAll("button")].find((candidate) => candidate.getAttribute("aria-label") === name);
  if (!match) throw new Error(`no button named ${name}`);
  return match;
}

describe("surface tab view", () => {
  it("hosts the plugin view for this surface tab", async () => {
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" isVisible decl={viewDecl} />);
    const frame = host!.querySelector("iframe");
    expect(frame?.getAttribute("data-plugin-id")).toBe("dash");
    const query = new URL(pluginDocument.mock.calls[pluginDocument.mock.calls.length - 1][0], "http://127.0.0.1:7686").searchParams;
    expect(query.has("token")).toBe(false);
    expect(frame!.getAttribute("src")).toBeNull();
    expect(query.get("surface")).toBe("board");
    expect(query.get("instance")).toBe("inst-1");
    expect(query.get("tab")).toBe("surface-tab");
    expect(host!.querySelector("section")?.getAttribute("aria-label")).toBe("Jev's Board");
  });


  it("focuses the owner tab from the owner link", async () => {
    const onFocusOwner = vi.fn();
    await mount(
      <SurfaceTabView tab={tab} backing={backing} ownerName="Jev" onFocusOwner={onFocusOwner} isVisible decl={viewDecl} />,
    );
    await act(async () => button("Open owner tab Jev").click());
    expect(onFocusOwner).toHaveBeenCalledTimes(1);
  });

  it("streams a stream surface over the tab's own transport with the owner's name", async () => {
    const { created, factory } = fakeTransports();
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" isVisible decl={streamDecl} createTransport={factory} />);
    expect(host!.querySelector("iframe")).toBeNull();
    expect(byRole("toolbar", "Jev's Board controls")).not.toBeNull();
    expect(created).toEqual([{ baseUrl: "http://127.0.0.1:7686", token: "token", tabId: "surface-tab" }]);
  });

  it("binds the live stream and controls to the selected daemon", async () => {
    vi.mocked(relayTarget).mockReturnValue({ id: "eight", name: "Eight" });
    const { created, factory } = fakeTransports();
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" isVisible decl={streamDecl} createTransport={factory} />);
    expect(created).toEqual([{ baseUrl: "http://127.0.0.1:7686", token: "token", tabId: "surface-tab", peerId: "eight" }]);
  });

  it("holds no stream while the stream tab is hidden", async () => {
    const { created, factory } = fakeTransports();
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" isVisible={false} decl={streamDecl} createTransport={factory} />);
    expect(byRole("toolbar", "Jev's Board controls")).not.toBeNull();
    expect(created).toHaveLength(1);
  });

  it("renders nothing while the declaration loads and explains an unsupported renderer", async () => {
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" isVisible decl={{ status: "loading" }} />);
    expect(host!.querySelector("iframe")).toBeNull();
    await act(async () => root!.render(
      <SurfaceTabView
        tab={tab}
        backing={backing}
        ownerName="Jev"
       
        isVisible
        decl={{ status: "ready", declared: { pluginId: "dash", surfaceId: "board", renderer: "stream", runtime: null, chrome: undefined } }}
      />,
    ));
    expect(host!.querySelector("[role='alert']")).not.toBeNull();
  });

  it("shows a quiet unavailable state instead of the plugin view when the plugin declares nothing", async () => {
    const browserBacking: SurfaceTabBacking = { ...backing, plugin_id: "browser", surface_id: "browser" };
    const browserTab: Tab = { ...tab, title: "Browser", backing: browserBacking };
    await mount(<SurfaceTabView tab={browserTab} backing={browserBacking} ownerName="Jev" isVisible decl={{ status: "ready", declared: null }} />);
    expect(host!.querySelector("iframe")).toBeNull();
    expect(host!.querySelector("[data-testid='surface-flow-line']")).toBeNull();
    const notice = host!.querySelector("[data-testid='surface-unavailable']")!;
    expect(notice.querySelector("details")).toBeNull();
  });

  it("keeps the lookup error behind a details disclosure and never mounts the plugin view", async () => {
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" isVisible decl={{ status: "failed", error: "HTTP 503" }} />);
    expect(host!.querySelector("iframe")).toBeNull();
    const notice = host!.querySelector("[data-testid='surface-unavailable']")!;
    expect(notice.querySelector("details code")?.textContent).toBe("HTTP 503");
  });

  it("retries the surface lookup once from the unavailable control", async () => {
    const onRetry = vi.fn();
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" isVisible decl={{ status: "failed", error: "connection refused" }} onRetry={onRetry} />);
    const button = [...host!.querySelectorAll("button")].find((candidate) => candidate.textContent === "Retry surface lookup")!;
    await act(async () => button.click());
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("offers no owner link when the owner is gone", async () => {
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName={null} onFocusOwner={vi.fn()} isVisible decl={viewDecl} />);
    const names = [...host!.querySelectorAll("button")].map((candidate) => candidate.getAttribute("aria-label"));
    expect(names).toEqual([]);
  });

  it("reads as a breadcrumb from the owner chip to the surface", async () => {
    const onFocusOwner = vi.fn();
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" onFocusOwner={onFocusOwner} isVisible decl={viewDecl} />);
    const crumbs = host!.querySelector("[data-testid='surface-breadcrumb']")!;
    expect(crumbs.textContent).toBe("Jev›Board");
    const chip = crumbs.querySelector("[data-testid='surface-owner-chip']")!;
    expect(chip.tagName).toBe("BUTTON");
    expect(crumbs.querySelector("[data-testid='surface-crumb'] [data-testid='surface-crumb-icon']")).not.toBeNull();
    await act(async () => (chip as HTMLButtonElement).click());
    expect(onFocusOwner).toHaveBeenCalledTimes(1);
  });

  it("shows the owner chip as plain text when the owner cannot be focused", async () => {
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" isVisible decl={viewDecl} />);
    expect(host!.querySelector("[data-testid='surface-owner-chip']")?.tagName).toBe("SPAN");
  });

  it("drops the owner crumb when the owner is gone", async () => {
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName={null} isVisible decl={viewDecl} />);
    expect(host!.querySelector("[data-testid='surface-breadcrumb']")?.textContent).toBe("Board");
  });

  it("runs the flow line once per agent verb and rests otherwise", async () => {
    vi.useFakeTimers();
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" isVisible decl={viewDecl} />);
    const line = () => host!.querySelector("[data-testid='surface-flow-line']")!;
    expect(line().getAttribute("data-state")).toBe("idle");
    await act(async () => recordSurfaceActivity({ tab_id: "surface-tab", verb: "click" }));
    expect(line().getAttribute("data-state")).toBe("driving");
    expect(line().querySelectorAll("[data-testid='surface-flow-pass']")).toHaveLength(1);
    await act(async () => vi.advanceTimersByTime(SURFACE_PULSE_MS));
    expect(line().getAttribute("data-state")).toBe("idle");
    expect(line().querySelector("[data-testid='surface-flow-pass']")).toBeNull();
  });

  it("names a visiting agent in the breadcrumb while it drives the owner's surface", async () => {
    vi.useFakeTimers();
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" isVisible decl={viewDecl} />);
    const crumbs = () => host!.querySelector("[data-testid='surface-breadcrumb']")!.textContent;
    await act(async () => recordSurfaceActivity({ tab_id: "surface-tab", verb: "click", actor: "QA" }));
    expect(crumbs()).toBe("Jev›Board· QA");
    await act(async () => recordSurfaceActivity({ tab_id: "surface-tab", verb: "goto" }));
    expect(crumbs()).toBe("Jev›Board");
  });

  it("opens the page in the system browser through the host open_url path", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    openInSystemBrowser("https://a.test/x");
    expect(vi.mocked(invoke).mock.calls).toEqual([["open_url", { url: "https://a.test/x" }]]);
    vi.mocked(invoke).mockReset();
  });

  it("shares Take control with the sidebar and stills the flow line", async () => {
    const { factory } = fakeTransports();
    await mount(<SurfaceTabView tab={tab} backing={backing} ownerName="Jev" isVisible decl={streamDecl} createTransport={factory} />);
    expect(surfaceControlStore.get("surface-tab")).toBe(false);
    await act(async () => button("Take control").click());
    expect(surfaceControlStore.get("surface-tab")).toBe(true);
    await act(async () => recordSurfaceActivity({ tab_id: "surface-tab", verb: "click" }));
    expect(host!.querySelector("[data-testid='surface-flow-line']")?.getAttribute("data-state")).toBe("controlled");
    expect(host!.querySelector("[data-testid='surface-flow-pass']")).toBeNull();
  });
});
