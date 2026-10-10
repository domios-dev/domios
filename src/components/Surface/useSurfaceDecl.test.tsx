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

import { daemonFetch } from "../../lib/webApi";
import { nextSurfaceDeclLookup, surfaceDeclRetryDelay, useSurfaceDecl } from "./useSurfaceDecl";
import type { SurfaceDeclLookup } from "./surfaceRenderPlan";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const browserRow = { plugin_id: "browser", surface_id: "browser", renderer: "stream", runtime: "browser" };
const json = (declared: unknown[]) => new Response(JSON.stringify({ declared, open: [] }), { status: 200 });
const fetchMock = vi.mocked(daemonFetch);

let root: Root | null = null;
let latest: SurfaceDeclLookup = { status: "loading" };

function Probe() {
  latest = useSurfaceDecl({ host: "127.0.0.1", port: 7686, token: "t" }, "browser", "browser");
  return null;
}

async function mount() {
  root = createRoot(document.createElement("div"));
  await act(async () => root!.render(<Probe />));
}

const advance = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

beforeEach(() => {
  vi.useFakeTimers();
  fetchMock.mockReset();
  listeners.clear();
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = null;
  vi.useRealTimers();
});

describe("useSurfaceDecl", () => {
  it("backs off 1s, 2s, 5s, then every 15s", () => {
    expect([0, 1, 2, 3, 9].map(surfaceDeclRetryDelay)).toEqual([1_000, 2_000, 5_000, 15_000, 15_000]);
  });

  it("retries a failed lookup and recovers when the plugin finishes loading", async () => {
    fetchMock
      .mockRejectedValueOnce(new Error("connection refused"))
      .mockResolvedValueOnce(json([]))
      .mockResolvedValueOnce(json([browserRow]));
    await mount();
    expect(latest).toEqual({ status: "failed", error: "connection refused" });
    await advance(999);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    await advance(1);
    expect(latest).toEqual({ status: "ready", declared: null });
    await advance(2_000);
    expect(latest.status === "ready" && latest.declared?.renderer).toBe("stream");
    await advance(60_000);
    expect(fetchMock).toHaveBeenCalledTimes(3);
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

  it("stops retrying once unmounted", async () => {
    fetchMock.mockRejectedValue(new Error("down"));
    await mount();
    await act(async () => root!.unmount());
    root = null;
    await advance(60_000);
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
