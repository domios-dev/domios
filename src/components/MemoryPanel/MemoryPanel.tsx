import { useCallback, useEffect, useRef, useState } from "react";
import { List, Network } from "lucide-react";
import { MemoryCanvas } from "./MemoryCanvas";
import { MemoryListTab } from "./MemoryListTab";
import { MemoryActivityTab } from "./MemoryActivityTab";
import { MemoryDreamsTab } from "./MemoryDreamsTab";
import { MemorySettingsTab } from "./MemorySettingsTab";
import { useGraphData } from "./useGraphData";
import { useActivityPulses } from "./useActivityPulses";
import { fetchStats, isTauriEnv, type MemStats } from "./memoryApi";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import type { ApiConfig, AppConfig } from "../../lib/types";

export type MemoryTab = "dreams" | "inspect" | "activity" | "settings";

function normalizeTab(t: MemoryTab | string | undefined): MemoryTab {
  if (t === "graph" || t === "list") return "inspect";
  if (t === "dreams" || t === "inspect" || t === "activity" || t === "settings") return t;
  return "dreams";
}

export interface MemoryInitialState {
  selectedId: string | null;
  feedVisible: boolean;
  workerPanelVisible: boolean;
  activeTab?: MemoryTab;
}

const TAB_BASE = "shrink-0 border-b-2 bg-transparent cursor-pointer capitalize";
const tabClass = (active: boolean, touch: boolean) =>
  `${TAB_BASE} ${touch ? "px-4 py-3 text-sm" : "px-3 py-2 text-xs"} ${
    active
      ? "border-[var(--ct-accent)] text-[var(--ct-fg)]"
      : "border-transparent text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
  }`;

const TABS: MemoryTab[] = ["dreams", "inspect", "activity", "settings"];

interface Props {
  api: ApiConfig;
  onClose: () => void;
  initialState?: MemoryInitialState;
  onFocusPane?: (paneId: string) => void;
  config?: AppConfig;
  onConfigUpdate?: (config: AppConfig) => Promise<void>;
}

export function MemoryPanel({ api, onClose, initialState, config, onConfigUpdate }: Props) {
  const touchUi = useTouchOptimizedUi();
  const [activeTab, setActiveTab] = useState<MemoryTab>(normalizeTab(initialState?.activeTab));
  const [inspectView, setInspectView] = useState<"graph" | "list">("graph");
  const {
    nodes,
    edges,
    loading,
    error,
    stats,
    updateNode,
    removeNode,
  } = useGraphData(api);
  const { pulses } = useActivityPulses(api);
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

  useEffect(() => {
    let active = true;
    const load = () => fetchStats(api).then((s) => { if (active) setMemStats(s); }).catch(() => {});
    load();
    let unlisten: (() => void) | null = null;
    if (isTauriEnv()) {
      import("../../lib/bridge").then(({ listen }) => {
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

  const fitRef = useRef<() => void>(() => {});
  const recenterRef = useRef<() => void>(() => {});
  const registerFit = useCallback((fn: () => void) => { fitRef.current = fn; }, []);
  const registerRecenter = useCallback((fn: () => void) => { recenterRef.current = fn; }, []);

  const handleNavigateToItem = useCallback((id: string) => {
    setActiveTab("inspect");
    setInspectView("list");
    setSelectedId(id);
  }, []);

  const handleViewInGraph = useCallback((id: string) => {
    setActiveTab("inspect");
    setInspectView("graph");
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
      if (e.key === "1") { e.preventDefault(); setActiveTab("dreams"); }
      else if (e.key === "2") { e.preventDefault(); setActiveTab("inspect"); }
      else if (e.key === "3") { e.preventDefault(); setActiveTab("activity"); }
      else if (e.key === "4") { e.preventDefault(); setActiveTab("settings"); }
      else if (e.key === "f" || e.key === "F") { if (activeTab === "inspect" && inspectView === "graph") { e.preventDefault(); fitRef.current(); } }
      else if (e.key === "0") { if (activeTab === "inspect" && inspectView === "graph") { e.preventDefault(); recenterRef.current(); } }
      else if (e.key === "p" || e.key === "P") { if (activeTab === "inspect" && inspectView === "graph") { e.preventDefault(); setPhysicsEnabled((v) => !v); } }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose, activeTab, inspectView]);

  return (
    <div className="absolute inset-0 flex flex-col bg-[var(--ct-bg)]">
      {/* Header with tabs */}
      <div className="flex items-center gap-0 px-4 py-0 text-[13px] bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[var(--ct-fg)] ct-hscroll whitespace-nowrap">
        <div className="flex items-center gap-3 mr-4 shrink-0">
          <strong>Memory</strong>
          <span className="text-[11px] opacity-70 max-sm:hidden">
            {stats.node_count} nodes &middot; {stats.edge_count} edges
          </span>
          {memStats && memStats.total_vectors < memStats.total_items && (
            <span className="text-[11px] text-[var(--ct-yellow)] flex items-center gap-1">
              <span className="inline-block w-1.5 h-1.5 rounded-full bg-[var(--ct-yellow)] animate-pulse" />
              indexing {memStats.total_vectors}/{memStats.total_items}
            </span>
          )}
          {memStats && memStats.total_vectors >= memStats.total_items && memStats.total_vectors > 0 && (
            <span className="text-[11px] text-[var(--ct-green)] opacity-60 max-sm:hidden">
              {memStats.total_vectors} vectors
            </span>
          )}
        </div>

        <div className="flex items-center gap-0 border-l border-[var(--ct-border)] pl-2">
          {TABS.map((tab) => (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={tabClass(activeTab === tab, touchUi)}
            >
              {tab}
            </button>
          ))}
        </div>
      </div>

      {/* Tab content */}
      <div className="relative flex-1 min-h-0">
        {activeTab === "dreams" && <MemoryDreamsTab api={api} />}
        {activeTab === "inspect" && inspectView === "list" && (
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
        {activeTab === "inspect" && inspectView === "graph" && loading && (
          <div className="absolute inset-0 flex items-center justify-center text-[var(--ct-muted-fg)]">
            Loading memory graph...
          </div>
        )}
        {activeTab === "inspect" && inspectView === "graph" && error && (
          <div className="absolute top-4 left-1/2 -translate-x-1/2 px-3 py-2 rounded bg-[var(--ct-red)] text-white text-xs">
            {error}
          </div>
        )}
        {/* Mount the canvas as soon as data is ready, even on other tabs, so
            Louvain seed + physics worker get a head start. Hide it (not
            unmount) when not active so sigma keeps its container size and the
            settled positions survive tab switches. */}
        {!loading && !error && (
          <div
            className={
              activeTab === "inspect" && inspectView === "graph"
                ? "absolute inset-0"
                : "absolute inset-0 invisible pointer-events-none"
            }
            aria-hidden={!(activeTab === "inspect" && inspectView === "graph")}
          >
            <MemoryCanvas
              api={api}
              nodes={nodes}
              edges={edges}
              pulses={pulses}
              updateNode={updateNode}
              removeNode={removeNode}
              registerFit={registerFit}
              registerRecenter={registerRecenter}
              selectedIdOverride={selectedId}
              onSelectedIdChange={setSelectedId}
              physicsEnabled={physicsEnabled}
            />
          </div>
        )}
        {activeTab === "inspect" && (
          <button
            type="button"
            onClick={() => setInspectView(inspectView === "graph" ? "list" : "graph")}
            className="absolute bottom-4 left-1/2 -translate-x-1/2 z-10 px-4 py-2 rounded-full bg-[var(--ct-bg)]/80 backdrop-blur-md border border-[var(--ct-border)] text-[11px] flex items-center gap-1.5 cursor-pointer hover:border-[var(--ct-accent)] shadow-lg text-[var(--ct-fg)]"
          >
            {inspectView === "graph" ? <List size={12} /> : <Network size={12} />}
            {inspectView === "graph" ? "List" : "Graph"}
          </button>
        )}
        {activeTab === "activity" && (
          <MemoryActivityTab
            api={api}
            nodes={nodes}
            onNavigateToItem={handleNavigateToItem}
          />
        )}
        {activeTab === "settings" && config && onConfigUpdate && (
          <div className="absolute inset-0 overflow-y-auto p-6">
            <MemorySettingsTab
              config={config}
              onUpdate={(section, patch) => {
                const next = { ...config, [section]: Object.assign({}, config[section], patch) };
                onConfigUpdate(next).catch(() => {});
              }}
            />
          </div>
        )}
      </div>
    </div>
  );
}
