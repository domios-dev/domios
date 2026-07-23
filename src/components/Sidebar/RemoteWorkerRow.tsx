import { useState } from "react";
import { Server } from "lucide-react";
import type { RemoteWorker } from "../../lib/types";

const STATUS_COLOR: Record<string, string> = {
  working: "var(--ct-green, #7ee787)",
  blocked: "var(--ct-red, #f85149)",
  partial: "var(--ct-yellow, #d29922)",
};

function relTime(ms: number): string {
  const d = Date.now() - ms;
  if (d < 60_000) return "just now";
  if (d < 3_600_000) return `${Math.floor(d / 60_000)}m ago`;
  if (d < 86_400_000) return `${Math.floor(d / 3_600_000)}h ago`;
  return `${Math.floor(d / 86_400_000)}d ago`;
}

/** A worker running on another mesh peer, shown as a read-only ghost node under
 *  its orchestrator. It has no local PTY — clicking expands its last report
 *  inline rather than focusing a terminal. The dashed left border + `@peer` chip
 *  signal "this one lives elsewhere". */
export function RemoteWorkerRow({
  worker,
  depth,
}: {
  worker: RemoteWorker;
  depth: number;
}) {
  const [open, setOpen] = useState(false);
  const color = STATUS_COLOR[worker.status] ?? "var(--ct-muted, #484f58)";
  return (
    <div style={{ paddingLeft: depth * 12 }}>
      <div
        data-testid="remote-worker-row"
        data-peer={worker.peer}
        onClick={() => setOpen((o) => !o)}
        title={`Remote worker on ${worker.peer} — ${worker.status}\n${worker.last_message}`}
        className="sidebar-tab sidebar-tab-dimmed group flex items-center gap-2 cursor-pointer pl-4 pr-2 py-1.5 hover:bg-[rgba(255,255,255,0.04)] transition-colors"
        style={{ borderLeft: "1px dashed var(--ct-border, #30363d)" }}
      >
        <Server size={12} strokeWidth={1.5} className="shrink-0 opacity-70" />
        <span className="flex-1 min-w-0 truncate text-[13px]">{worker.title}</span>
        <span className="shrink-0 text-[10px] px-1 rounded bg-[var(--ct-hover)] text-[var(--ct-muted)]">
          @{worker.peer}
        </span>
        <span
          className={`inline-block w-2 h-2 rounded-full shrink-0 ${worker.status === "working" ? "animate-pulse" : ""}`}
          style={{ backgroundColor: color }}
          title={worker.status}
        />
      </div>
      {open && (
        <div
          style={{ paddingLeft: depth * 12 + 24 }}
          className="text-[11px] text-[var(--ct-muted)] pr-2 pb-1.5 space-y-0.5"
        >
          <div>
            runs on <strong className="text-[var(--ct-fg)]">{worker.peer}</strong> ·
            pane {worker.remote_pane.slice(0, 8)}
          </div>
          {worker.model && <div>model {worker.model}</div>}
          <div>
            status {worker.status} · {relTime(worker.last_seen_ms)}
          </div>
          {worker.last_message && (
            <div className="whitespace-pre-wrap break-words opacity-90">
              {worker.last_message}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
