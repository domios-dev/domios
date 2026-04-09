import { useCallback, useEffect, useRef, useState } from "react";
import { MemoryCanvas } from "./MemoryCanvas";
import { MemoryActivityFeed } from "./MemoryActivityFeed";
import { MemoryWorkerPanel } from "./MemoryWorkerPanel";
import { useGraphData } from "./useGraphData";
import { useActivityPulses } from "./useActivityPulses";
import { workerFocusStore } from "./workerFocusStore";
import { navigationStore } from "../Navigation/navigationStore";
import type { ApiConfig, GraphWorkerInfo } from "../../lib/types";

export interface MemoryInitialState {
  selectedId: string | null;
  feedVisible: boolean;
  workerPanelVisible: boolean;
}

const BTN_CLASS =
  "bg-transparent border border-[var(--ct-border)] text-inherit px-2.5 py-1 rounded text-xs cursor-pointer";
const BTN_ACTIVE_CLASS =
  "bg-[var(--ct-hover)] border border-[var(--ct-border)] text-inherit px-2.5 py-1 rounded text-xs cursor-pointer";

interface Props {
  api: ApiConfig;
  onClose: () => void;
  initialState?: MemoryInitialState;
  onFocusPane?: (paneId: string) => void;
}

export function MemoryPanel({ api, onClose, initialState, onFocusPane }: Props) {
  const { nodes, edges, loading, error, stats, updateNode, bulkUpdatePositions, removeNode } = useGraphData(api);
  const { pulses, recentEvents } = useActivityPulses(api);
  const [feedVisible, setFeedVisible] = useState(initialState?.feedVisible ?? false);
  const [workerPanelVisible, setWorkerPanelVisible] = useState(
    initialState?.workerPanelVisible ?? false,
  );
  const [selectedId, setSelectedId] = useState<string | null>(
    initialState?.selectedId ?? null,
  );
  const [physicsEnabled, setPhysicsEnabled] = useState(true);

  const handleOpenWorker = useCallback(
    (worker: GraphWorkerInfo) => {
      navigationStore.set({
        view: "memory-panel",
        memoryState: {
          panX: 0,
          panY: 0,
          scale: 1,
          selectedNodeId: selectedId,
          feedVisible,
          workerPanelVisible,
        },
        label: "Memory Graph",
      });
      workerFocusStore.focus(worker.pane_id);
      onFocusPane?.(worker.pane_id);
      onClose();
    },
    [selectedId, feedVisible, workerPanelVisible, onFocusPane, onClose],
  );
  const fitRef = useRef<() => void>(() => {});
  const recenterRef = useRef<() => void>(() => {});

  const registerFit = useCallback((fn: () => void) => {
    fitRef.current = fn;
  }, []);
  const registerRecenter = useCallback((fn: () => void) => {
    recenterRef.current = fn;
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
      if (e.key === "f" || e.key === "F") {
        e.preventDefault();
        fitRef.current();
      } else if (e.key === "0") {
        e.preventDefault();
        recenterRef.current();
      } else if (e.key === "a" || e.key === "A") {
        e.preventDefault();
        setFeedVisible((v) => !v);
      } else if (e.key === "w" || e.key === "W") {
        e.preventDefault();
        setWorkerPanelVisible((v) => !v);
      } else if (e.key === "p" || e.key === "P") {
        e.preventDefault();
        setPhysicsEnabled((v) => !v);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  return (
    <div className="absolute inset-0 flex flex-col bg-[var(--ct-bg)]">
      <div className="flex items-center justify-between gap-3 px-4 py-2.5 text-[13px] bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[var(--ct-fg)]">
        <div>
          <strong>Memory</strong>
          <span className="ml-3 opacity-70">
            {stats.node_count} nodes · {stats.edge_count} edges
          </span>
        </div>
        <div className="flex gap-1.5">
          <button
            onClick={() => setFeedVisible((v) => !v)}
            className={feedVisible ? BTN_ACTIVE_CLASS : BTN_CLASS}
            title="Toggle activity feed (A)"
          >
            Activity (A)
          </button>
          <button
            onClick={() => setWorkerPanelVisible((v) => !v)}
            className={workerPanelVisible ? BTN_ACTIVE_CLASS : BTN_CLASS}
            title="Toggle workers panel (W)"
          >
            Workers (W)
          </button>
        </div>
      </div>
      <div className="relative flex-1 min-h-0">
        {loading && (
          <div className="absolute inset-0 flex items-center justify-center text-[var(--ct-muted-fg)]">
            Loading memory graph…
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
        {feedVisible && (
          <MemoryActivityFeed events={recentEvents} onSelect={setSelectedId} />
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
