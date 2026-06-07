import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ApiConfig, GraphWorkerInfo } from "../../lib/types";
import { authHeaders, baseUrl } from "../MemoryPanel/memoryApi";
import { EventRow } from "./EventRenderer";
import { ClaudeStreamParser, mergeAssistantMessages } from "./streamParser";
import { useAgentRunStream } from "./useAgentRunStream";

interface Props {
  worker: GraphWorkerInfo;
  api: ApiConfig;
  onClose: () => void;
}

const TERMINAL_STATUSES = new Set(["done", "error", "orphaned"]);

export function AgentRunLogPopup({ worker, api, onClose }: Props) {
  const title = worker.intent.replace(/^extract:/, "");
  const subtitle = [worker.agent_provider, worker.model].filter(Boolean).join(" / ");
  const isTerminal = TERMINAL_STATUSES.has(worker.status);
  const livePaneId = isTerminal ? null : worker.pane_id;

  const { events: liveEvents, rawLines, connected, error } = useAgentRunStream({
    api,
    paneId: livePaneId,
    enabled: !!livePaneId,
  });

  const [fallbackText, setFallbackText] = useState<string | null>(null);
  useEffect(() => {
    if (!isTerminal) return;
    fetch(`${baseUrl(api)}/api/mem/workers/${encodeURIComponent(worker.worker_id)}/log`, {
      headers: authHeaders(api),
    })
      .then((r) => (r.ok ? r.text() : Promise.reject(r.statusText)))
      .then(setFallbackText)
      .catch(() => {});
  }, [api, worker.worker_id, isTerminal]);

  // Run the fallback log through the SAME stream parser as live runs so completed sessions render with collapsed thinking, tool calls, and tool
  // results.
  const fallbackEvents = useMemo(() => {
    if (!fallbackText) return [];
    const parser = new ClaudeStreamParser();
    const parsed = parser.feed(fallbackText);
    return mergeAssistantMessages(parsed.events);
  }, [fallbackText]);

  const events = liveEvents.length > 0 ? liveEvents : fallbackEvents;

  const [showRaw, setShowRaw] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const backdropRef = useRef<HTMLDivElement>(null);

  const stickyBottomRef = useRef(true);
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onScroll = () => {
      const dist = el.scrollHeight - el.scrollTop - el.clientHeight;
      stickyBottomRef.current = dist < 60;
    };
    el.addEventListener("scroll", onScroll);
    return () => el.removeEventListener("scroll", onScroll);
  }, []);
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    if (stickyBottomRef.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [events, rawLines.length]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  const handleBackdropClick = (e: React.MouseEvent) => {
    if (e.target === backdropRef.current) onClose();
  };

  const showFallback =
    events.length === 0 && rawLines.length === 0 && !!fallbackText;

  return createPortal(
    <div
      ref={backdropRef}
      onClick={handleBackdropClick}
      className="fixed inset-0 z-[60] flex items-center justify-center backdrop-blur-sm"
      style={{ background: "rgba(0,0,0,0.65)" }}
    >
      <div
        className="flex flex-col bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded-lg shadow-2xl"
        style={{ width: "min(820px, 90vw)", height: "min(640px, 85vh)" }}
      >
        <div className="flex items-center justify-between px-4 py-2.5 border-b border-[var(--ct-border)]">
          <div className="flex items-center gap-2 min-w-0">
            <span
              className="text-xs shrink-0"
              style={{ color: worker.color ?? "var(--ct-muted-fg)" }}
            >
              {isTerminal ? "✓" : "◕"}
            </span>
            <span className="text-xs font-medium truncate text-[var(--ct-fg)]">
              {title}
            </span>
            {subtitle && (
              <span className="text-[9px] text-[var(--ct-muted-fg)] shrink-0">
                {subtitle}
              </span>
            )}
            <StatusBadge status={worker.status} />
            {!isTerminal && connected && (
              <span className="text-[9px] text-[var(--ct-green)] shrink-0">
                live
              </span>
            )}
          </div>
          <button
            onClick={onClose}
            className="bg-transparent border-0 text-[var(--ct-muted-fg)] cursor-pointer text-sm px-1 hover:text-[var(--ct-fg)]"
          >
            ✕
          </button>
        </div>

        <div
          ref={scrollRef}
          className="flex-1 overflow-auto px-4 py-3 text-[11px] leading-relaxed text-[var(--ct-fg)] font-mono"
          style={{ background: "var(--ct-terminal-bg, var(--ct-bg))" }}
        >
          {error && (
            <div className="text-[var(--ct-red)] mb-2">stream error: {error}</div>
          )}

          {events.length === 0 && rawLines.length === 0 && !showFallback && (
            <div className="text-[var(--ct-muted-fg)]">
              {livePaneId
                ? connected
                  ? "Waiting for first event…"
                  : "Connecting…"
                : worker.status.toLowerCase() === "queued"
                  ? "Queued — waiting for worker slot…"
                  : "Waiting for spawn…"}
            </div>
          )}

          {events.map((ev, i) => (
            <EventRow key={i} event={ev} />
          ))}

          {showFallback && (
            <pre className="whitespace-pre-wrap break-words text-[10px] text-[var(--ct-muted-fg)]">
              {fallbackText}
            </pre>
          )}

          {rawLines.length > 0 && events.length === 0 && (
            <div className="mt-3 border-t border-[var(--ct-border)] pt-2">
              <button
                type="button"
                onClick={() => setShowRaw((v) => !v)}
                className="bg-transparent border-0 text-[10px] text-[var(--ct-muted-fg)] cursor-pointer p-0 hover:text-[var(--ct-fg)]"
              >
                {showRaw ? "▾" : "▸"} raw shell output ({rawLines.length} lines)
              </button>
              {showRaw && (
                <pre className="mt-1 whitespace-pre-wrap break-words text-[10px] text-[var(--ct-muted-fg)] opacity-80">
                  {rawLines.map((l) => l.text).join("\n")}
                </pre>
              )}
            </div>
          )}
        </div>

        {worker.report && (
          <div className="px-4 py-2 border-t border-[var(--ct-border)] text-[10px] text-[var(--ct-muted-fg)]">
            <strong>Report:</strong> {worker.report}
          </div>
        )}
      </div>
    </div>,
    document.body,
  );
}

function StatusBadge({ status }: { status: string }) {
  const lc = status.toLowerCase();
  const color =
    lc === "done" || lc === "success"
      ? "var(--ct-green)"
      : lc === "error" || lc === "failed"
        ? "var(--ct-red)"
        : lc === "orphaned" || lc === "cancelled"
          ? "var(--ct-muted-fg)"
          : lc === "running"
            ? "var(--ct-yellow)"
            : "var(--ct-accent)";
  return (
    <span
      className="text-[9px] px-1.5 py-[1px] rounded-full shrink-0"
      style={{ color, border: `1px solid ${color}` }}
    >
      {status}
    </span>
  );
}
