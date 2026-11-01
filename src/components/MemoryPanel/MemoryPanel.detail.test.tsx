import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { ApiConfig } from "../../lib/types";
import { MemoryPanel } from "./MemoryPanel";
import { MemoryNodeDetail } from "./MemoryNodeDetail";
import { MemoryFriendsContext } from "./MemoryProvenance";
import type { MemNode } from "./types";

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
vi.mock("../Settings/useAccount", () => ({ useAccount: () => ({ model: { kind: "signed_in", identity: { uid: "owner-fixture" } } }) }));
vi.mock("../Neighbors/useNeighbors", () => ({ useNeighbors: () => ({ list: { friends: [{ uid: "friend-fixture", friendship_id: "friendship-fixture", displayName: "Alex" }] } }) }));
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

it("shows the Neighbors display name in list, detail and pinned graph detail, and omits provenance for owner items", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const api = { host: "127.0.0.1", port: 7686, token: "fixture" } as ApiConfig;
  const neighbor = { kind: "neighbor", account_uid: "friend-fixture", friendship_id: "friendship-fixture" } as const;
  const authored = fixture as MemNode;
  authored.author = neighbor;
  try {
    await act(async () => root.render(<MemoryPanel api={api} onClose={() => {}} onOpenAgentsSettings={() => {}} initialState={{ activeTab: "inspect", inspectView: "list", selectedId: null, feedVisible: false, workerPanelVisible: false }} />));
    expect(host.querySelector('[data-testid="memory-list-item"] [data-testid="memory-provenance"]')?.getAttribute("aria-label")).toBe("From Alex · neighbor");
    await act(async () => host.querySelector<HTMLElement>('[data-testid="memory-list-item"] > div')!.click());
    expect(host.querySelector('[role="dialog"] [data-testid="memory-provenance"]')?.getAttribute("aria-label")).toBe("From Alex · neighbor");

    const renderDetail = (node: MemNode, friends = [{ uid: "friend-fixture", friendship_id: "friendship-fixture", displayName: "Alex" }]) =>
      <MemoryFriendsContext.Provider value={friends}><MemoryNodeDetail api={api} node={node} edges={[]} nodes={new Map()} onUpdate={() => {}} onDelete={() => {}} onSelectNode={() => {}} /></MemoryFriendsContext.Provider>;
    await act(async () => root.render(renderDetail({ ...authored, pinned: true })));
    expect(host.querySelector('[data-testid="memory-provenance"]')?.getAttribute("aria-label")).toBe("From Alex · neighbor");
    expect(host.textContent).not.toContain("Pinned position");
    expect(host.textContent).not.toContain("friend-fixture");
    await act(async () => root.render(renderDetail(authored, [])));
    expect(host.querySelector('[data-testid="memory-provenance"]')?.getAttribute("aria-label")).toBe("From a neighbor");
    await act(async () => root.render(renderDetail({ ...authored, author: { kind: "owner" } })));
    expect(host.querySelector('[data-testid="memory-provenance"]')).toBeNull();
    authored.author = { kind: "owner" };
    await act(async () => root.render(<MemoryPanel api={api} onClose={() => {}} onOpenAgentsSettings={() => {}} initialState={{ activeTab: "inspect", inspectView: "list", selectedId: null, feedVisible: false, workerPanelVisible: false }} />));
    expect(host.querySelector('[data-testid="memory-provenance"]')).toBeNull();
    await act(async () => host.querySelector<HTMLElement>('[data-testid="memory-list-item"] > div')!.click());
    expect(host.querySelector('[role="dialog"] [data-testid="memory-provenance"]')).toBeNull();
  } finally {
    delete authored.author;
    await act(async () => root.unmount());
    host.remove();
  }
});
