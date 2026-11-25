import { MemoryFriendsProvider } from "./MemoryProvenance";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { listen } from "../../lib/bridge";
import { memoryListControlsVisible } from "./memoryDetailModel";
import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import { List, Network } from "lucide-react";
import { CanvasToolbar } from "@codeterm/chat-ui/CanvasToolbar";
import { MemorySnapshotCanvas } from "./MemorySnapshotCanvas";
import { MemoryListTab } from "./MemoryListTab";
import { MemoryActivityTab } from "./MemoryActivityTab";
import { MemoryDreamsTab } from "./MemoryDreamsTab";
import { MemorySettingsTab } from "./MemorySettingsTab";
import { useGraphData } from "./useGraphData";
import { useSnapshotGraph } from "./useSnapshotGraph";
import { useActivityPulses } from "./useActivityPulses";
import { fetchStats, type MemStats } from "./memoryApi";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import type { ApiConfig, AppConfig } from "../../lib/types";
import { UI_TEXT } from "../shared/design";
import { Loader } from "../shared/Loader";
import { LiquidTabRail } from "../shared/LiquidTabRail";
import "./memoryPanelChrome.css";
import { memoryStatsSummary } from "./memoryStatsSummary";

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
  inspectView?: "graph" | "list";
}

const TABS: MemoryTab[] = ["dreams", "inspect", "activity", "settings"];
const INSPECT_VIEWS = [{ value: "list", label: "List", Icon: List }, { value: "graph", label: "Graph", Icon: Network }] as const;

function InspectViewToggle({ view, onToggle }: { view: "list" | "graph"; onToggle: () => void }) {
  return (
    <div className="memory-neutral-segments" role="group" aria-label="Inspect view">
      {INSPECT_VIEWS.map(({ value, label, Icon }) => (
        <button
          key={value}
          type="button"
          aria-label={label}
          aria-pressed={value === view}
          disabled={value === view}
          data-testid={value !== view ? "memory-inspect-view-toggle" : undefined}
          data-memory-inspect-view={value !== view ? view : undefined}
          onClick={onToggle}
          className="memory-neutral-segment"
        >
          <Icon size={14} aria-hidden="true" />
          <span className="max-sm:hidden">{label}</span>
        </button>
      ))}
    </div>
  );
}

interface Props {
  api: ApiConfig;
  onClose: () => void;
  initialState?: MemoryInitialState;
  onFocusTab?: (tabId: string) => void;
  config?: AppConfig;
  onConfigUpdate?: (config: AppConfig) => Promise<void>;
  onOpenAgentsSettings: () => void;
}

function MemoryPanelContent({ api, onClose, initialState, config, onConfigUpdate, onOpenAgentsSettings }: Props) {
  const [detailOpen, setDetailOpen] = useState(false);
  const touchUi = useTouchOptimizedUi();
  const [activeTab, setActiveTab] = useState<MemoryTab>(normalizeTab(initialState?.activeTab));
  const [inspectView, setInspectView] = useState<"graph" | "list">("graph");
  const {
    nodes,
    updateNode,
    removeNode,
  } = useGraphData(api);
  const snapshot = useSnapshotGraph(api);
  const graphVisible = activeTab === "inspect" && inspectView === "graph";
  const refreshSnapshot = snapshot.refresh;
  const graphWasVisible = useRef(graphVisible);
  useEffect(() => {
    // Mount already fetches fresh; only a return to the graph needs another pull.
    if (graphVisible && !graphWasVisible.current) void refreshSnapshot();
    graphWasVisible.current = graphVisible;
  }, [graphVisible, refreshSnapshot]);
  const { pulses } = useActivityPulses(api);
  const [selectedId, setSelectedId] = useState<string | null>(
    initialState?.selectedId ?? null,
  );

  useEffect(() => {
    if (!initialState) return;
    setSelectedId(initialState.selectedId ?? null);
    if (initialState.activeTab) setActiveTab(normalizeTab(initialState.activeTab));
    if (initialState.inspectView) setInspectView(initialState.inspectView);
  }, [initialState]);

  const configPropRef = useRef(config);
  configPropRef.current = config;
  const latestConfigRef = useRef(config);
  const inFlightConfigWritesRef = useRef(0);
  useEffect(() => {
    if (inFlightConfigWritesRef.current === 0) {
      latestConfigRef.current = config;
    }
  }, [config]);

  const handleMemoryConfigUpdate = useCallback(
    <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => {
      if (!onConfigUpdate) return;
      const base = latestConfigRef.current ?? configPropRef.current;
      if (!base) return;
      const next = { ...base, [section]: Object.assign({}, base[section], patch) };
      latestConfigRef.current = next;
      inFlightConfigWritesRef.current += 1;
      onConfigUpdate(next)
        .catch(() => {
          if (inFlightConfigWritesRef.current === 1) {
            latestConfigRef.current = configPropRef.current;
          }
        })
        .finally(() => {
          inFlightConfigWritesRef.current = Math.max(0, inFlightConfigWritesRef.current - 1);
        });
    },
    [onConfigUpdate],
  );

  const [memStats, setMemStats] = useState<MemStats | null>(null);

  useEffect(() => {
    let active = true;
    const load = () => fetchStats(api).then((s) => { if (active) setMemStats(s); }).catch(() => {});
    load();
    let unlisten: (() => void) | null = null;
    listen<{ op: string }>("mem-activity", (ev) => {
      if (!active) return;
      if (ev.payload?.op === "write" || ev.payload?.op === "delete") load();
    }).then((fn) => {
      if (!active) { fn(); return; }
      unlisten = fn;
    }).catch(() => {});
    return () => { active = false; if (unlisten) unlisten(); };
  }, [api.host, api.port, api.token]);

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
      if (detailOpen) return;
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
  }, [onClose, activeTab, inspectView, detailOpen]);

  const snapshotError =
    snapshot.error ??
    (snapshot.status === "unavailable" ? "Memory snapshot is unavailable." : null);
  const showSnapshotEmpty =
    snapshot.status === "ready" && snapshot.index != null && snapshot.index.byId.size === 0;
  const graphViewToggle = <InspectViewToggle view="graph" onToggle={() => setInspectView("list")} />;

  return (
    <div
      className="absolute inset-0 flex flex-col bg-[var(--ct-bg)] pt-[var(--ct-safe-top,0px)] pb-[var(--ct-safe-bottom,0px)]"
      data-testid="memory-panel"
      data-memory-active-tab={activeTab}
    >
        <div
          className={`memory-panel-header ${detailOpen ? "max-sm:!hidden" : ""}`}
        >
          <h1>Memory</h1>
          <span className="memory-panel-stats" data-testid="memory-stats-summary">
            {memoryStatsSummary(
              memStats ? memStats.total_entities + memStats.total_items : (snapshot.stats?.node_count ?? 0),
              memStats ? memStats.total_relations : (snapshot.stats?.edge_count ?? 0),
              memStats?.total_vectors ?? 0,
            )}
          </span>

          <div className="memory-panel-tabs">
            {detailOpen ? (
              <div
                className="ct-liquid-tab-rail"
                inert
                style={{ "--rail-count": TABS.length, "--rail-index": TABS.indexOf(activeTab) } as CSSProperties}
              >
                <span className="ct-liquid-tab-rail__indicator" />
                {TABS.map(tab => (
                  <span
                    key={tab}
                    aria-selected={tab === activeTab}
                    className={`ct-liquid-tab-rail__tab flex !cursor-default items-center justify-center ${tab === activeTab ? "" : "opacity-60"}`}
                  >
                    {tab}
                  </span>
                ))}
              </div>
            ) : (
              <LiquidTabRail
                value={activeTab}
                options={TABS}
                onChange={setActiveTab}
                label="Memory sections"
                touch={touchUi}
                testIdPrefix="memory-tab"
              />
            )}
          </div>
        </div>
      {/* Tab content */}
      <div className="@container/memory relative flex-1 min-h-0">
        {activeTab === "dreams" && <MemoryDreamsTab api={api} onViewInGraph={handleViewInGraph} />}
        {activeTab === "inspect" && inspectView === "list" && (
          <MemoryListTab
            api={api}
            nodes={nodes}
            selectedId={selectedId}
            onSelectItem={setSelectedId}
            onDetailOpenChange={setDetailOpen}
            onViewInGraph={handleViewInGraph}
            viewToggle={<InspectViewToggle view="list" onToggle={() => setInspectView("graph")} />}
            updateNode={updateNode}
            removeNode={removeNode}
          />
        )}
        {activeTab === "inspect" && inspectView === "graph" &&
          snapshot.status === "loading" && !snapshot.index && (
          <Loader variant="overlay" data-testid="memory-graph-loading" label="Loading memory graph" />
        )}
        {activeTab === "inspect" && inspectView === "graph" && snapshotError && (
          <div data-testid="memory-graph-error"><InlineErrorNotice error={snapshotError} operation="loading memory" blocking /></div>
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
              metadataNodes={nodes}
              epoch={snapshot.epoch}
              pulses={pulses}
              onRefresh={() => void snapshot.refresh()}
              toolbarView={memoryListControlsVisible(detailOpen) ? graphViewToggle : undefined}
              registerFit={registerFit}
              registerRecenter={registerRecenter}
              selectedIdOverride={selectedId}
              onSelectedIdChange={setSelectedId}
            />
          </div>
        )}
        {activeTab === "inspect" && inspectView === "graph" && (!snapshot.index || showSnapshotEmpty) && memoryListControlsVisible(detailOpen) && <CanvasToolbar label="Memory graph controls" onRefresh={() => void snapshot.refresh()} refreshTestId="memory-graph-refresh" view={graphViewToggle} />}
        {activeTab === "activity" && (
          <MemoryActivityTab
            api={api}
            config={config}
            nodes={nodes}
            onNavigateToItem={handleNavigateToItem}
          />
        )}
        {activeTab === "settings" && config && onConfigUpdate && (
          <div className="absolute inset-0 overflow-y-auto px-4 py-4 sm:p-6">
            <MemorySettingsTab
              config={config}
              onOpenAgentsSettings={onOpenAgentsSettings}
              onUpdate={handleMemoryConfigUpdate}
            />
          </div>
        )}
      </div>
    </div>
  );
}

export function MemoryPanel(props: Props) {
  return <MemoryFriendsProvider><MemoryPanelContent {...props} /></MemoryFriendsProvider>;
}
