import { useCallback, useEffect, useRef, useState } from "react";
import type { ApiConfig, GraphWorkerInfo } from "../../lib/types";
import { fetchWorkerLog } from "./memoryApi";

interface Props {
  worker: GraphWorkerInfo;
  api: ApiConfig;
  onClose: () => void;
}

export function WorkerLogPopup({ worker, api, onClose }: Props) {
  const [log, setLog] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const scrollRef = useRef<HTMLPreElement>(null);
  const backdropRef = useRef<HTMLDivElement>(null);

  const isRunning = !["done", "error", "orphaned"].includes(worker.status);

  const loadLog = useCallback(async () => {
    try {
      const text = await fetchWorkerLog(api, worker.worker_id);
      setLog(text);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [api, worker.worker_id]);

  useEffect(() => {
    loadLog();
    if (!isRunning) return;
    // Poll for live output while running
    const interval = setInterval(loadLog, 2000);
    return () => clearInterval(interval);
  }, [loadLog, isRunning]);

  // Auto-scroll to bottom on new content
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [log]);

  // Close on Escape
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  // Close on click outside
  const handleBackdropClick = (e: React.MouseEvent) => {
    if (e.target === backdropRef.current) onClose();
  };

  const intent = worker.intent.replace(/^extract:/, "");
  const statusLabel = isRunning ? `${worker.status}…` : worker.status;
  const statusColor: Record<string, string> = {
    starting: "var(--ct-muted-fg)", reading: "var(--ct-accent)",
    analyzing: "var(--ct-accent)", writing: "var(--ct-yellow)",
    done: "var(--ct-green)", error: "var(--ct-red)", orphaned: "var(--ct-muted-fg)",
  };
  const badgeColor = statusColor[worker.status] ?? "var(--ct-muted-fg)";

  return (
    <div
      ref={backdropRef}
      onClick={handleBackdropClick}
      className="fixed inset-0 z-50 flex items-center justify-center"
      style={{ background: "rgba(0,0,0,0.4)" }}
    >
      <div
        className="flex flex-col bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded-lg shadow-2xl"
        style={{ width: "min(720px, 85vw)", height: "min(520px, 75vh)" }}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-2.5 border-b border-[var(--ct-border)]">
          <div className="flex items-center gap-2 min-w-0">
            <span className="text-xs shrink-0" style={{ color: worker.color }}>
              {isRunning ? "◕" : "✓"}
            </span>
            <span className="text-xs font-medium truncate text-[var(--ct-fg)]">
              {intent}
            </span>
            {(worker.agent_provider || worker.model) && (
              <span className="text-[9px] text-[var(--ct-muted-fg)] shrink-0">
                {[worker.agent_provider, worker.model].filter(Boolean).join(" / ")}
              </span>
            )}
            <span
              className="text-[9px] px-1.5 py-[1px] rounded-full shrink-0"
              style={{ color: badgeColor, border: `1px solid ${badgeColor}` }}
            >
              {statusLabel}
            </span>
          </div>
          <button
            onClick={onClose}
            className="bg-transparent border-0 text-[var(--ct-muted-fg)] cursor-pointer text-sm px-1 hover:text-[var(--ct-fg)]"
          >
            ✕
          </button>
        </div>

        {/* Log content */}
        <pre
          ref={scrollRef}
          className="flex-1 overflow-auto m-0 px-4 py-3 text-[11px] leading-relaxed text-[var(--ct-fg)] font-mono whitespace-pre-wrap break-words"
          style={{ background: "var(--ct-terminal-bg, var(--ct-bg))" }}
        >
          {error && (
            <span className="text-[var(--ct-red)]">Error: {error}</span>
          )}
          {log === null && !error && (
            <span className="text-[var(--ct-muted-fg)]">Loading…</span>
          )}
          {log}
        </pre>

        {/* Footer */}
        {worker.report && (
          <div className="px-4 py-2 border-t border-[var(--ct-border)] text-[10px] text-[var(--ct-muted-fg)]">
            <strong>Report:</strong> {worker.report}
          </div>
        )}
      </div>
    </div>
  );
}
