import { useCallback, useEffect, useRef, useState } from "react";
import { List, Network, RefreshCw } from "lucide-react";
import { MemorySnapshotCanvas } from "./MemorySnapshotCanvas";
import { MemoryListTab } from "./MemoryListTab";
import { MemoryActivityTab } from "./MemoryActivityTab";
import { MemoryDreamsTab } from "./MemoryDreamsTab";
import { MemorySettingsTab } from "./MemorySettingsTab";
import { useGraphData } from "./useGraphData";
import { useSnapshotGraph } from "./useSnapshotGraph";
import { useActivityPulses } from "./useActivityPulses";
import { fetchStats, isTauriEnv, type MemStats } from "./memoryApi";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import type { ApiConfig, AppConfig } from "../../lib/types";
import { tabClass, UI_BUTTON, UI_PANEL, UI_TEXT } from "../shared/design";

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
    updateNode,
    removeNode,
  } = useGraphData(api);
  const snapshot = useSnapshotGraph(api);
  const { pulses } = useActivityPulses(api);
  const [selectedId, setSelectedId] = useState<string | null>(
    initialState?.selectedId ?? null,
  );

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
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose, activeTab, inspectView]);

  const snapshotError =
    snapshot.error ??
    (snapshot.status === "unavailable" ? "Memory snapshot is unavailable." : null);
  const showSnapshotEmpty =
    snapshot.status === "ready" && snapshot.index != null && snapshot.index.byId.size === 0;

  return (
    <div
      className="absolute inset-0 flex flex-col bg-[var(--ct-bg)] pt-[var(--ct-safe-top,0px)] pb-[var(--ct-safe-bottom,0px)]"
      data-testid="memory-panel"
      data-memory-active-tab={activeTab}
    >
      {/* Header with tabs */}
      <div
        className={`${UI_PANEL.toolbar} gap-0 px-4 py-0 bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[var(--ct-fg)] ct-hscroll whitespace-nowrap`}
        style={touchUi ? { height: "auto", minHeight: "3.25rem" } : undefined}
      >
        <div className="flex items-center gap-3 mr-4 shrink-0">
          <strong>Memory</strong>
          <span className="text-[11px] opacity-70 max-sm:hidden" data-testid="memory-stats-summary">
            {/* Use fresh memStats (refreshes on mem-activity) — the cached graph
                snapshot is keyed by layout_version and goes stale after edge edits. */}
            {memStats
              ? memStats.total_entities + memStats.total_items
              : (snapshot.stats?.node_count ?? 0)}{" "}
            nodes &middot;{" "}
            {memStats ? memStats.total_relations : (snapshot.stats?.edge_count ?? 0)} edges
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

        <div className="flex items-center gap-1 border-l border-[var(--ct-border)] pl-3">
          {TABS.map((tab) => (
            <button
              key={tab}
              data-testid={`memory-tab-${tab}`}
              onClick={() => setActiveTab(tab)}
              className={tabClass(activeTab === tab, touchUi)}
              aria-pressed={activeTab === tab}
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
        {activeTab === "inspect" && inspectView === "graph" &&
          snapshot.status === "loading" && !snapshot.index && (
          <div className="absolute inset-0 flex items-center justify-center">
            <span className={UI_TEXT.meta} data-testid="memory-graph-loading">Loading memory graph...</span>
          </div>
        )}
        {activeTab === "inspect" && inspectView === "graph" && snapshotError && (
          <div className="absolute top-4 left-1/2 -translate-x-1/2 px-3 py-2 rounded bg-[var(--ct-red)] text-white text-xs" data-testid="memory-graph-error" role="alert">
            {snapshotError}
          </div>
        )}
        {activeTab === "inspect" && inspectView === "graph" && showSnapshotEmpty && (
          <div className="absolute inset-0 flex items-center justify-center">
            <span className={UI_TEXT.meta} data-testid="memory-graph-empty">No memory graph data yet.</span>
          </div>
        )}
        {snapshot.index && !showSnapshotEmpty && (
          <div
            className={
              activeTab === "inspect" && inspectView === "graph"
                ? "absolute inset-0"
                : "absolute inset-0 invisible pointer-events-none"
            }
            aria-hidden={!(activeTab === "inspect" && inspectView === "graph")}
            data-testid="memory-graph-view"
          >
            <MemorySnapshotCanvas
              api={api}
              index={snapshot.index}
              pulses={pulses}
              registerFit={registerFit}
              registerRecenter={registerRecenter}
              selectedIdOverride={selectedId}
              onSelectedIdChange={setSelectedId}
            />
          </div>
        )}
        {activeTab === "inspect" && (
          <div className="absolute bottom-4 left-1/2 -translate-x-1/2 z-10 flex items-center gap-1.5">
            <button
              type="button"
              data-testid="memory-inspect-view-toggle"
              data-memory-inspect-view={inspectView}
              onClick={() => setInspectView(inspectView === "graph" ? "list" : "graph")}
              className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg} gap-1.5 backdrop-blur-md shadow-lg`}
            >
              {inspectView === "graph" ? <List size={12} /> : <Network size={12} />}
              {inspectView === "graph" ? "List" : "Graph"}
            </button>
            {inspectView === "graph" && (
              <button
                type="button"
                title="Drop cached layout and refetch"
                data-testid="memory-graph-refresh"
                onClick={() => void snapshot.refresh()}
                className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg} gap-1.5 backdrop-blur-md shadow-lg`}
              >
                <RefreshCw size={12} />
              </button>
            )}
          </div>
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
