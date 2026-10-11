import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const listeners = new Map<string, (event: { payload: unknown }) => void>();
vi.mock("../../lib/bridge", () => ({
  listen: vi.fn((event: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(event, handler);
    return Promise.resolve(() => listeners.delete(event));
  }),
}));
vi.mock("../../lib/webApi", () => ({ daemonFetch: vi.fn() }));

import { listen } from "../../lib/bridge";
import { daemonFetch } from "../../lib/webApi";
import { nextSurfaceDeclLookup, useSurfaceDecl } from "./useSurfaceDecl";
import type { SurfaceDeclLookup } from "./surfaceRenderPlan";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const browserRow = { plugin_id: "browser", surface_id: "browser", renderer: "stream", runtime: "browser" };
const json = (declared: unknown[]) => new Response(JSON.stringify({ declared, open: [] }), { status: 200 });
const fetchMock = vi.mocked(daemonFetch);

let root: Root | null = null;
let latest: SurfaceDeclLookup = { status: "loading" };

function Probe({ generation = 0 }: { generation?: number }) {
  latest = useSurfaceDecl({ host: "127.0.0.1", port: 7686, token: "t" }, "browser", "browser", generation);
  return null;
}

async function mount() {
  root = createRoot(document.createElement("div"));
  await act(async () => root!.render(<Probe />));
}


beforeEach(() => {
  fetchMock.mockReset();
  listeners.clear();
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = null;
});

describe("useSurfaceDecl", () => {
  it("waits for the contribution subscription before taking its initial snapshot", async () => {
    let subscribed!: (unlisten: () => void) => void;
    vi.mocked(listen).mockImplementationOnce(() => new Promise((resolve) => { subscribed = resolve; }));
    fetchMock.mockResolvedValueOnce(json([browserRow]));
    await mount();
    expect(fetchMock).not.toHaveBeenCalled();
    expect(latest).toEqual({ status: "loading" });
    await act(async () => subscribed(() => undefined));
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(latest.status === "ready" && latest.declared?.runtime).toBe("browser");
  });

  it("recovers a refused HTTP lookup only on an explicit retry", async () => {
    fetchMock.mockRejectedValueOnce(new Error("connection refused")).mockResolvedValueOnce(json([browserRow]));
    await mount();
    expect(latest).toEqual({ status: "failed", error: "connection refused" });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    await act(async () => root!.render(<Probe generation={1} />));
    expect(latest.status === "ready" && latest.declared?.renderer).toBe("stream");
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("refetches when plugin contributions change", async () => {
    fetchMock.mockResolvedValueOnce(json([])).mockResolvedValueOnce(json([browserRow]));
    await mount();
    expect(latest).toEqual({ status: "ready", declared: null });
    await act(async () => listeners.get("contributions-changed")!({ payload: { kind: "plugin_view:browser" } }));
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(latest.status === "ready" && latest.declared?.runtime).toBe("browser");
  });

  it("keeps a rendering surface through a failed refetch", async () => {
    fetchMock.mockResolvedValueOnce(json([browserRow])).mockRejectedValueOnce(new Error("HTTP 502"));
    await mount();
    await act(async () => listeners.get("contributions-changed")!({ payload: { kind: "agent_provider" } }));
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(latest.status === "ready" && latest.declared?.pluginId).toBe("browser");
  });

  it("unsubscribes from contribution changes when unmounted", async () => {
    fetchMock.mockRejectedValue(new Error("down"));
    await mount();
    await act(async () => root!.unmount());
    root = null;
    expect(listeners.has("contributions-changed")).toBe(false);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("only a failure after a declared surface is held back", () => {
    const declared: SurfaceDeclLookup = { status: "ready", declared: { pluginId: "p", surfaceId: "s", renderer: "view", runtime: null, chrome: undefined } };
    const failed: SurfaceDeclLookup = { status: "failed", error: "x" };
    expect(nextSurfaceDeclLookup(declared, failed)).toBe(declared);
    expect(nextSurfaceDeclLookup({ status: "ready", declared: null }, failed)).toBe(failed);
    expect(nextSurfaceDeclLookup(declared, { status: "ready", declared: null })).toEqual({ status: "ready", declared: null });
  });
});
