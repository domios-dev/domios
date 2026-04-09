import { memo, useState } from "react";
import type {
  ApiConfig,
  GraphWorkerInfo,
  GraphWorkerStatus,
  SpawnWorkerReq,
} from "../../lib/types";
import { useGraphWorkers } from "./useGraphWorkers";
import { MemorySpawnWorkerDialog } from "./MemorySpawnWorkerDialog";

interface Props {
  api: ApiConfig;
  onOpenWorker: (worker: GraphWorkerInfo) => void;
  onClose: () => void;
}

const STATUS_FILL: Record<GraphWorkerStatus, number> = {
  starting: 1,
  reading: 2,
  analyzing: 3,
  writing: 4,
  done: 5,
  error: 5,
  orphaned: 0,
};

const STATUS_COLOR: Record<GraphWorkerStatus, string> = {
  starting: "var(--ct-muted-fg, #888)",
  reading: "var(--ct-accent, #3b82f6)",
  analyzing: "var(--ct-accent, #3b82f6)",
  writing: "var(--ct-yellow, #eab308)",
  done: "var(--ct-green, #22c55e)",
  error: "var(--ct-red, #ef4444)",
  orphaned: "var(--ct-muted, #555)",
};

function ProgressBar({ status }: { status: GraphWorkerStatus }) {
  const fill = STATUS_FILL[status];
  const color = STATUS_COLOR[status];
  const cells: React.ReactNode[] = [];
  for (let i = 0; i < 5; i++) {
    cells.push(
      <div
        key={i}
        className="flex-1 h-1.5 rounded-[1px]"
        style={{ background: i < fill ? color : "var(--ct-border)" }}
      />,
    );
  }
  return <div className="flex gap-0.5">{cells}</div>;
}

interface BubbleProps {
  worker: GraphWorkerInfo;
  onOpen: () => void;
  onKill: () => void;
}

const WorkerBubble = memo(function WorkerBubble({
  worker,
  onOpen,
  onKill,
}: BubbleProps) {
  const [expanded, setExpanded] = useState(false);
  const isDone =
    worker.status === "done" ||
    worker.status === "error" ||
    worker.status === "orphaned";

  return (
    <div
      className="border border-[var(--ct-border)] rounded-md p-2.5 mb-2 bg-[var(--ct-surface)] text-[11px]"
      style={{ opacity: isDone ? 0.6 : 1 }}
    >
      <div className="flex items-center gap-1.5 mb-1.5">
        <span
          className="w-2 h-2 rounded-full shrink-0"
          style={{ background: worker.color }}
        />
        <span
          className="flex-1 font-mono text-[10px] text-[var(--ct-muted-fg)] overflow-hidden text-ellipsis whitespace-nowrap"
          title={worker.worker_id}
        >
          {worker.worker_id}
        </span>
        <button
          onClick={onKill}
          className="bg-transparent border-0 text-[var(--ct-muted-fg)] cursor-pointer text-xs px-1"
          title="Kill worker"
        >
          ✕
        </button>
      </div>
      <div className="mb-1">
        <span className="text-[var(--ct-muted-fg)]">Intent: </span>
        <span>{worker.intent}</span>
      </div>
      <div className="mb-1.5">
        <div className="flex justify-between mb-[3px]">
          <span className="text-[var(--ct-muted-fg)]">Status</span>
          <span style={{ color: STATUS_COLOR[worker.status] }}>{worker.status}</span>
        </div>
        <ProgressBar status={worker.status} />
      </div>
      {worker.progress_text && (
        <div className="mb-1 text-[var(--ct-muted-fg)]">{worker.progress_text}</div>
      )}
      <div className="text-[var(--ct-muted-fg)] mb-0.5">By: {worker.spawned_by}</div>
      <div className="text-[var(--ct-muted-fg)] mb-2">
        Touched: {worker.touched_nodes.length}
      </div>
      <button
        onClick={onOpen}
        className="w-full bg-transparent border border-[var(--ct-border)] text-inherit px-2 py-1 rounded text-[11px] cursor-pointer"
      >
        → Open terminal
      </button>
      {worker.report && (
        <div className="mt-2">
          <button
            onClick={() => setExpanded((v) => !v)}
            className="bg-transparent border-0 text-[var(--ct-accent)] cursor-pointer text-[11px] p-0"
          >
            {expanded ? "▼" : "▶"} Report
          </button>
          {expanded && (
            <pre className="mt-1 p-1.5 bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded text-[10px] whitespace-pre-wrap break-words max-h-[200px] overflow-auto">
              {worker.report}
            </pre>
          )}
        </div>
      )}
    </div>
  );
});

function MemoryWorkerPanelImpl({ api, onOpenWorker, onClose }: Props) {
  const { workers, loading, error, spawn, kill, refresh } = useGraphWorkers(api);
  const [dialogOpen, setDialogOpen] = useState(false);

  const handleSpawn = async (req: SpawnWorkerReq) => {
    await spawn(req);
  };

  const handleKill = async (workerId: string) => {
    if (!window.confirm(`Kill worker ${workerId}?`)) return;
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
      <div className="absolute top-0 right-0 bottom-0 w-[300px] flex flex-col z-20 bg-[var(--ct-sidebar-bg)] border-l border-[var(--ct-border)] text-[var(--ct-fg)]">
        <div className="px-3 py-2.5 border-b border-[var(--ct-border)] flex items-center justify-between gap-2">
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
        <div className="flex-1 overflow-auto p-2.5">
          {loading && (
            <div className="text-[11px] text-[var(--ct-muted-fg)]">Loading…</div>
          )}
          {error && !loading && (
            <div className="p-2 bg-[var(--ct-red)] text-white rounded text-[11px] mb-2">
              <div>{error}</div>
              <button onClick={() => refresh()} className={`${btnClass} mt-1 !text-white`}>
                Retry
              </button>
            </div>
          )}
          {!loading && !error && workers.length === 0 && (
            <div className="text-[11px] text-[var(--ct-muted-fg)] text-center p-5">
              No workers yet — spawn one
            </div>
          )}
          {workers.map((w) => (
            <WorkerBubble
              key={w.worker_id}
              worker={w}
              onOpen={() => onOpenWorker(w)}
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
    </>
  );
}

export const MemoryWorkerPanel = memo(MemoryWorkerPanelImpl);
