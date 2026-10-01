import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiConfig } from "../../lib/types";
import type { GraphSnapshot } from "./snapshotTypes";
import { type UseSnapshotGraphResult, useSnapshotGraph } from "./useSnapshotGraph";

const api = vi.hoisted(() => ({
  fetchSnapshot: vi.fn(),
  fetchSnapshotDelta: vi.fn(),
  loadCachedSnapshot: vi.fn(),
  saveCachedSnapshot: vi.fn(),
  clearCachedSnapshot: vi.fn(),
}));

vi.mock("./memoryApi", async (original) => ({
  ...(await original<typeof import("./memoryApi")>()),
  fetchSnapshot: api.fetchSnapshot,
  fetchSnapshotDelta: api.fetchSnapshotDelta,
}));
vi.mock("./snapshotCache", () => ({
  loadCachedSnapshot: api.loadCachedSnapshot,
  saveCachedSnapshot: api.saveCachedSnapshot,
  clearCachedSnapshot: api.clearCachedSnapshot,
}));
vi.mock("../../lib/bridge", () => ({ listen: () => Promise.resolve(() => {}) }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const API: ApiConfig = { host: "127.0.0.1", port: 7686, token: "t" } as ApiConfig;

function snapshot(ids: string[], seq: number): GraphSnapshot {
  return {
    layout_version: 1,
    computed_at: "2026-01-01T00:00:00Z",
    seq,
    nodes: ids.map((id) => ({
      id,
      x: 0,
      y: 0,
      kind: "fact",
      importance: 3,
      cluster_id: 0,
      has_vector: true,
      pinned: false,
    })),
    clusters: [],
    cluster_edges: [],
    stats: { node_count: ids.length, cluster_count: 0, edge_count: 0, vectorized_without_position: 0 },
  };
}

let host: HTMLElement;
let root: Root;
let latest: UseSnapshotGraphResult | null;

function Probe() {
  latest = useSnapshotGraph(API);
  return null;
}

async function flush() {
  for (let i = 0; i < 5; i += 1) await act(async () => {});
}

describe("useSnapshotGraph freshness", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.saveCachedSnapshot.mockResolvedValue(undefined);
    api.clearCachedSnapshot.mockResolvedValue(undefined);
    latest = null;
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
  });

  it("paints the cached graph, then replaces it with a full fetch", async () => {
    api.loadCachedSnapshot.mockResolvedValue(snapshot(["stale"], 1));
    api.fetchSnapshot.mockResolvedValue(snapshot(["fresh"], 2));

    act(() => root.render(<Probe />));
    await flush();

    expect(api.fetchSnapshot).toHaveBeenCalledTimes(1);
    expect(api.fetchSnapshotDelta).not.toHaveBeenCalled();
    expect([...(latest?.index?.byId.keys() ?? [])]).toEqual(["fresh"]);
    expect(latest?.epoch).toBe(1);
  });

  it("keeps the cached graph when the fresh fetch fails", async () => {
    api.loadCachedSnapshot.mockResolvedValue(snapshot(["cached"], 1));
    api.fetchSnapshot.mockRejectedValue(new Error("offline"));

    act(() => root.render(<Probe />));
    await flush();

    expect(latest?.status).toBe("ready");
    expect(latest?.error).toBeNull();
    expect([...(latest?.index?.byId.keys() ?? [])]).toEqual(["cached"]);
  });

  it("refresh pulls a full snapshot and bumps the epoch", async () => {
    api.loadCachedSnapshot.mockResolvedValue(null);
    api.fetchSnapshot.mockResolvedValueOnce(snapshot(["a"], 1)).mockResolvedValueOnce(snapshot(["a", "b"], 2));

    act(() => root.render(<Probe />));
    await flush();
    const refresh = latest?.refresh;
    await act(async () => {
      await latest?.refresh();
    });

    expect(api.clearCachedSnapshot).toHaveBeenCalledTimes(1);
    expect([...(latest?.index?.byId.keys() ?? [])]).toEqual(["a", "b"]);
    expect(latest?.epoch).toBe(2);
    expect(latest?.refresh).toBe(refresh);
  });
});
