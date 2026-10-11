import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { ApiConfig } from "../../lib/types";
import { MemoryPanel } from "./MemoryPanel";

const fixture = vi.hoisted(() => ({
  id: "fixture-id", title: "Fixture name", kind: "note", body: "Body", tags: [],
  summary: null, area: null, importance: null, has_vector: false, has_body: true,
}));
vi.mock("./MemorySnapshotCanvas", () => ({ MemorySnapshotCanvas: () => null }));
vi.mock("./useGraphData", () => ({ useGraphData: () => ({ nodes: new Map([[fixture.id, fixture]]), updateNode: vi.fn(), removeNode: vi.fn() }) }));
vi.mock("./useSnapshotGraph", () => ({ useSnapshotGraph: () => ({ status: "ready", index: null, refresh: vi.fn() }) }));
vi.mock("./useActivityPulses", () => ({ useActivityPulses: () => ({ pulses: new Map() }) }));
vi.mock("../../hooks/useTouchOptimizedUi", () => ({ useTouchOptimizedUi: () => true }));
vi.mock("../../lib/bridge", () => ({ listen: async () => () => {} }));
vi.mock("./memoryApi", () => ({
  fetchStats: async () => ({ total_entities: 0, total_items: 1, total_relations: 0, total_vectors: 1 }),
  fetchItemFull: async () => fixture,
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("the detail owns its header and returns to Inspect without leaving Memory", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const onClose = vi.fn();
  const api = { host: "127.0.0.1", port: 7686, token: "fixture" } as ApiConfig;
  try {
    await act(async () => root.render(<MemoryPanel api={api} onClose={onClose} onOpenAgentsSettings={() => {}} initialState={{ activeTab: "inspect", inspectView: "list", selectedId: null, feedVisible: false, workerPanelVisible: false }} />));
    expect(host.querySelector('[role="tablist"][aria-label="Memory sections"]')).not.toBeNull();
    await act(async () => host.querySelector<HTMLElement>('[data-testid="memory-list-item"] > div')!.click());
    expect(host.querySelector('[role="tablist"][aria-label="Memory sections"]')).toBeNull();
    expect(host.querySelector('[data-testid="memory-inspect-view-toggle"]')).toBeNull();
    expect(host.querySelector('[data-testid="memory-list-search"]')).toBeNull();
    const back = host.querySelector<HTMLButtonElement>('button[aria-label="Back to Memory Inspect"]');
    expect(back).not.toBeNull();
    await act(async () => back!.click());
    expect(host.querySelector('[role="tablist"][aria-label="Memory sections"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="memory-list-search"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="memory-panel"]')?.getAttribute("data-memory-active-tab")).toBe("inspect");
    expect(onClose).not.toHaveBeenCalled();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
