import { memo, useState } from "react";
import type {
  ApiConfig,
  GraphWorkerInfo,
  GraphWorkerStatus,
  SpawnWorkerReq,
} from "../../lib/types";
import { useGraphWorkers } from "./useGraphWorkers";
import { MemorySpawnWorkerDialog } from "./MemorySpawnWorkerDialog";
import { WorkerLogPopup } from "./WorkerLogPopup";

interface Props {
  api: ApiConfig;
  onOpenWorker: (worker: GraphWorkerInfo) => void;
  onClose: () => void;
}

const STATUS_COLOR: Record<GraphWorkerStatus, string> = {
  starting: "var(--ct-muted-fg, #888)",
  reading: "var(--ct-accent, #3b82f6)",
  analyzing: "var(--ct-accent, #3b82f6)",
  writing: "var(--ct-yellow, #eab308)",
  done: "var(--ct-green, #22c55e)",
  error: "var(--ct-red, #ef4444)",
  orphaned: "var(--ct-muted, #555)",
};

const STATUS_ICON: Record<GraphWorkerStatus, string> = {
  starting: "◌",
  reading: "◑",
  analyzing: "◕",
  writing: "◉",
  done: "✓",
  error: "✕",
  orphaned: "○",
};

interface RowProps {
  worker: GraphWorkerInfo;
  onOpen: () => void;
  onKill: () => void;
}

const WorkerRow = memo(function WorkerRow({ worker, onOpen, onKill }: RowProps) {
  const isDone =
    worker.status === "done" ||
    worker.status === "error" ||
    worker.status === "orphaned";

  const intent = worker.intent.replace(/^extract:/, "");

  return (
    <div style={{ opacity: isDone ? 0.55 : 1 }}>
      <div
        className="flex items-center gap-1.5 px-2 py-[5px] cursor-pointer rounded hover:bg-[var(--ct-hover-bg)]"
        onClick={onOpen}
        title={isDone ? "View log" : "View live output"}
      >
        <span
          className="shrink-0 text-[10px] w-3 text-center"
          style={{ color: worker.color }}
        >
          {STATUS_ICON[worker.status]}
        </span>
        <span className="flex-1 text-[11px] overflow-hidden text-ellipsis whitespace-nowrap min-w-0">
          {intent}
        </span>
        {worker.model && (
          <span className="text-[9px] text-[var(--ct-muted-fg)] shrink-0">
            {worker.model}
          </span>
        )}
        <span
          className="text-[9px] px-1.5 py-[1px] rounded-full shrink-0"
          style={{
            color: STATUS_COLOR[worker.status],
            border: `1px solid ${STATUS_COLOR[worker.status]}`,
          }}
        >
          {worker.status}
        </span>
        {!isDone && (
          <button
            onClick={(e) => { e.stopPropagation(); onKill(); }}
            className="bg-transparent border-0 text-[var(--ct-muted-fg)] cursor-pointer text-[10px] px-0.5 hover:text-[var(--ct-red)]"
            title="Kill"
          >
            ✕
          </button>
        )}
      </div>
    </div>
  );
});

function MemoryWorkerPanelImpl({ api, onClose }: Props) {
  const { workers, loading, error, spawn, kill, refresh } = useGraphWorkers(api);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [logWorker, setLogWorker] = useState<GraphWorkerInfo | null>(null);

  const handleSpawn = async (req: SpawnWorkerReq) => {
    await spawn(req);
  };

  const handleKill = async (workerId: string) => {
    try {
      await kill(workerId);
      await refresh();
    } catch (e) {
      window.alert(`Failed to kill: ${e instanceof Error ? e.message : e}`);
    }
  };

  const btnClass =
    "bg-transparent border border-[var(--ct-border)] text-inherit px-2 py-[3px] rounded text-[11px] cursor-pointer";

  return (
    <>
      <div className="absolute top-0 right-0 bottom-0 w-[260px] flex flex-col z-20 bg-[var(--ct-sidebar-bg)] border-l border-[var(--ct-border)] text-[var(--ct-fg)]">
        <div className="px-3 py-2 border-b border-[var(--ct-border)] flex items-center justify-between gap-2">
          <strong className="text-xs">Workers ({workers.length})</strong>
          <div className="flex gap-1">
            <button onClick={() => setDialogOpen(true)} className={btnClass} title="Spawn worker">
              + Spawn
            </button>
            <button onClick={onClose} className={btnClass} title="Hide panel (W)">
              ×
            </button>
          </div>
        </div>
        <div className="flex-1 overflow-auto py-1">
          {loading && (
            <div className="text-[11px] text-[var(--ct-muted-fg)] px-3 py-2">Loading…</div>
          )}
          {error && !loading && (
            <div className="p-2 mx-2 bg-[var(--ct-red)] text-white rounded text-[11px] mb-1">
              {error}
              <button onClick={() => refresh()} className={`${btnClass} mt-1 !text-white`}>
                Retry
              </button>
            </div>
          )}
          {!loading && !error && workers.length === 0 && (
            <div className="text-[11px] text-[var(--ct-muted-fg)] text-center py-6">
              No workers yet — spawn one
            </div>
          )}
          {workers.map((w) => (
            <WorkerRow
              key={w.worker_id}
              worker={w}
              onOpen={() => setLogWorker(w)}
              onKill={() => handleKill(w.worker_id)}
            />
          ))}
        </div>
      </div>
      {dialogOpen && (
        <MemorySpawnWorkerDialog
          onSpawn={handleSpawn}
          onClose={() => setDialogOpen(false)}
        />
      )}
      {logWorker && (
        <WorkerLogPopup
          worker={logWorker}
          api={api}
          onClose={() => setLogWorker(null)}
        />
      )}
    </>
  );
}

export const MemoryWorkerPanel = memo(MemoryWorkerPanelImpl);
