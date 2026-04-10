import { useCallback, useEffect, useRef, useState } from "react";
import { MemoryCanvas } from "./MemoryCanvas";
import { MemoryListTab } from "./MemoryListTab";
import { MemoryActivityTab } from "./MemoryActivityTab";
import { MemoryWorkerPanel } from "./MemoryWorkerPanel";
import { useGraphData } from "./useGraphData";
import { useActivityPulses } from "./useActivityPulses";
import { workerFocusStore } from "./workerFocusStore";
import { navigationStore } from "../Navigation/navigationStore";
import { fetchStats, isTauriEnv, type MemStats } from "./memoryApi";
import type { ApiConfig, GraphWorkerInfo } from "../../lib/types";

export type MemoryTab = "list" | "graph" | "activity";

export interface MemoryInitialState {
  selectedId: string | null;
  feedVisible: boolean;
  workerPanelVisible: boolean;
  activeTab?: MemoryTab;
}

const TAB_BASE =
  "px-3 py-2 text-xs border-b-2 bg-transparent cursor-pointer capitalize";
const TAB_ACTIVE = `${TAB_BASE} border-[var(--ct-accent)] text-[var(--ct-fg)]`;
const TAB_INACTIVE = `${TAB_BASE} border-transparent text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]`;

const BTN_CLASS =
  "bg-transparent border border-[var(--ct-border)] text-inherit px-2.5 py-1 rounded text-xs cursor-pointer";
const BTN_ACTIVE_CLASS =
  "bg-[var(--ct-hover)] border border-[var(--ct-border)] text-inherit px-2.5 py-1 rounded text-xs cursor-pointer";

const TABS: MemoryTab[] = ["list", "graph", "activity"];

interface Props {
  api: ApiConfig;
  onClose: () => void;
  initialState?: MemoryInitialState;
  onFocusPane?: (paneId: string) => void;
}

export function MemoryPanel({ api, onClose, initialState, onFocusPane }: Props) {
  const [activeTab, setActiveTab] = useState<MemoryTab>(initialState?.activeTab ?? "list");
  const { nodes, edges, loading, error, stats, updateNode, bulkUpdatePositions, removeNode } = useGraphData(api);
  const { pulses } = useActivityPulses(api);
  const [workerPanelVisible, setWorkerPanelVisible] = useState(
    initialState?.workerPanelVisible ?? false,
  );
  const [selectedId, setSelectedId] = useState<string | null>(
    initialState?.selectedId ?? null,
  );

  // Physics starts disabled — show force layout result instantly
  const [physicsEnabled, setPhysicsEnabled] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setPhysicsEnabled(true), 1500);
    return () => clearTimeout(timer);
  }, []);

  const [memStats, setMemStats] = useState<MemStats | null>(null);

  // Stats via Tauri events
  useEffect(() => {
    let active = true;
    const load = () => fetchStats(api).then((s) => { if (active) setMemStats(s); }).catch(() => {});
    load();
    let unlisten: (() => void) | null = null;
    if (isTauriEnv()) {
      import("@tauri-apps/api/event").then(({ listen }) => {
        listen<{ op: string }>("mem-activity", (ev) => {
          if (!active) return;
          if (ev.payload?.op === "write" || ev.payload?.op === "delete") load();
        }).then((fn) => {
          if (!active) { fn(); return; }
          unlisten = fn;
        }).catch(() => {});
      });
    }
    return () => { active = false; if (unlisten) unlisten(); };
  }, [api]);

  const handleOpenWorker = useCallback(
    (worker: GraphWorkerInfo) => {
      navigationStore.set({
        view: "memory-panel",
        memoryState: {
          panX: 0, panY: 0, scale: 1,
          selectedNodeId: selectedId,
          feedVisible: false,
          workerPanelVisible,
        },
        label: "Memory Graph",
      });
      workerFocusStore.focus(worker.pane_id);
      onFocusPane?.(worker.pane_id);
      onClose();
    },
    [selectedId, workerPanelVisible, onFocusPane, onClose],
  );

  const fitRef = useRef<() => void>(() => {});
  const recenterRef = useRef<() => void>(() => {});
  const registerFit = useCallback((fn: () => void) => { fitRef.current = fn; }, []);
  const registerRecenter = useCallback((fn: () => void) => { recenterRef.current = fn; }, []);

  const handleNavigateToItem = useCallback((id: string) => {
    setActiveTab("list");
    setSelectedId(id);
  }, []);

  const handleViewInGraph = useCallback((id: string) => {
    setActiveTab("graph");
    setSelectedId(id);
  }, []);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const inField =
        e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (e.key === "Escape") {
        if (inField) return;
        e.preventDefault();
        onClose();
        return;
      }
      if (inField) return;
      if (e.key === "1") { e.preventDefault(); setActiveTab("list"); }
      else if (e.key === "2") { e.preventDefault(); setActiveTab("graph"); }
      else if (e.key === "3") { e.preventDefault(); setActiveTab("activity"); }
      else if (e.key === "w" || e.key === "W") { e.preventDefault(); setWorkerPanelVisible((v) => !v); }
      else if (e.key === "f" || e.key === "F") { if (activeTab === "graph") { e.preventDefault(); fitRef.current(); } }
      else if (e.key === "0") { if (activeTab === "graph") { e.preventDefault(); recenterRef.current(); } }
      else if (e.key === "p" || e.key === "P") { if (activeTab === "graph") { e.preventDefault(); setPhysicsEnabled((v) => !v); } }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose, activeTab]);

  return (
    <div className="absolute inset-0 flex flex-col bg-[var(--ct-bg)]">
      {/* Header with tabs */}
      <div className="flex items-center gap-0 px-4 py-0 text-[13px] bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[var(--ct-fg)]">
        <div className="flex items-center gap-3 mr-4">
          <strong>Memory</strong>
          <span className="text-[11px] opacity-70">
            {stats.node_count} nodes &middot; {stats.edge_count} edges
          </span>
          {memStats && memStats.total_vectors < memStats.total_items && (
            <span className="text-[11px] text-[var(--ct-yellow)] flex items-center gap-1">
              <span className="inline-block w-1.5 h-1.5 rounded-full bg-[var(--ct-yellow)] animate-pulse" />
              indexing {memStats.total_vectors}/{memStats.total_items}
            </span>
          )}
          {memStats && memStats.total_vectors >= memStats.total_items && memStats.total_vectors > 0 && (
            <span className="text-[11px] text-[var(--ct-green)] opacity-60">
              {memStats.total_vectors} vectors
            </span>
          )}
        </div>

        <div className="flex items-center gap-0 border-l border-[var(--ct-border)] pl-2">
          {TABS.map((tab) => (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={activeTab === tab ? TAB_ACTIVE : TAB_INACTIVE}
            >
              {tab}
            </button>
          ))}
        </div>

        <div className="flex gap-1.5 ml-auto">
          <button
            onClick={() => setWorkerPanelVisible((v) => !v)}
            className={workerPanelVisible ? BTN_ACTIVE_CLASS : BTN_CLASS}
            title="Toggle workers (W)"
          >
            Workers (W)
          </button>
          <button onClick={onClose} className={BTN_CLASS} title="Close (Esc)">
            &times;
          </button>
        </div>
      </div>

      {/* Tab content */}
      <div className="relative flex-1 min-h-0">
        {activeTab === "list" && (
          <MemoryListTab
            api={api}
            nodes={nodes}
            selectedId={selectedId}
            onSelectItem={setSelectedId}
            onViewInGraph={handleViewInGraph}
            updateNode={updateNode}
            removeNode={removeNode}
          />
        )}
        {activeTab === "graph" && (
          <>
            {loading && (
              <div className="absolute inset-0 flex items-center justify-center text-[var(--ct-muted-fg)]">
                Loading memory graph...
              </div>
            )}
            {error && (
              <div className="absolute top-4 left-1/2 -translate-x-1/2 px-3 py-2 rounded bg-[var(--ct-red)] text-white text-xs">
                {error}
              </div>
            )}
            {!loading && !error && (
              <MemoryCanvas
                api={api}
                nodes={nodes}
                edges={edges}
                pulses={pulses}
                updateNode={updateNode}
                bulkUpdatePositions={bulkUpdatePositions}
                removeNode={removeNode}
                registerFit={registerFit}
                registerRecenter={registerRecenter}
                selectedIdOverride={selectedId}
                onSelectedIdChange={setSelectedId}
                physicsEnabled={physicsEnabled}
              />
            )}
          </>
        )}
        {activeTab === "activity" && (
          <MemoryActivityTab
            api={api}
            nodes={nodes}
            onNavigateToItem={handleNavigateToItem}
          />
        )}
        {workerPanelVisible && (
          <MemoryWorkerPanel
            api={api}
            onOpenWorker={handleOpenWorker}
            onClose={() => setWorkerPanelVisible(false)}
          />
        )}
      </div>
    </div>
  );
}
