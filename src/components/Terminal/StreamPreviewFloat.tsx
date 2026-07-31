import { useEffect, useState } from "react";

interface StreamPreviewFloatProps {
  rows: readonly string[];
  working: boolean;
  statusLine?: string | null;
  elapsedMs?: number | null;
}

function formatElapsed(ms?: number | null): string {
  if (!ms || ms < 1000) return "";
  const s = Math.floor(ms / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
}

const BAR_HEIGHT = 34;

/**
 * Slim single-line activity bar rendered IN FLOW as the last child of the
 * chat message list: it can never overlap settled messages, and its height
 * is constant while streaming (one truncated line), so the chat never jumps
 * mid-stream. Shows the latest cleaned preview line with a pulsing dot,
 * elapsed timer, and an indeterminate sweep along the bottom edge. Collapses
 * height→0 and unmounts at turn end — the real message is already above it.
 */
export function StreamPreviewFloat({ rows, working, statusLine, elapsedMs }: StreamPreviewFloatProps) {
  const show = working || rows.length > 0;
  const [mounted, setMounted] = useState(show);
  useEffect(() => {
    if (show) setMounted(true);
  }, [show]);
  if (!mounted) return null;

  const lastLine = rows.length > 0 ? rows[rows.length - 1] : statusLine ?? "";
  const elapsed = formatElapsed(elapsedMs);
  return (
    <div
      className="w-full max-w-4xl min-w-0 mx-auto overflow-hidden transition-all duration-200"
      style={{ height: show ? BAR_HEIGHT : 0, opacity: show ? 1 : 0 }}
      onTransitionEnd={() => {
        if (!show) setMounted(false);
      }}
      data-testid="chat-stream-preview"
      data-float="bar"
      role="status"
      aria-label="Streaming activity"
    >
      <div
        className="relative flex items-center gap-2 overflow-hidden rounded-md px-2.5"
        style={{
          height: BAR_HEIGHT,
          background: "color-mix(in srgb, var(--ct-surface, #161b22) 72%, transparent)",
          backdropFilter: "blur(10px)",
          WebkitBackdropFilter: "blur(10px)",
          borderLeft: "2px solid var(--ct-accent, #58a6ff)",
          color: "var(--ct-muted-fg)",
        }}
      >
        <span
          className="inline-block h-2 w-2 flex-none animate-pulse rounded-full"
          style={{ background: "var(--ct-accent, #58a6ff)" }}
        />
        <span className="flex-none font-mono text-xs">Working</span>
        <span
          className="min-w-0 flex-1 truncate whitespace-nowrap font-mono text-xs opacity-80"
          data-testid="chat-stream-preview-line"
        >
          {lastLine}
        </span>
        {elapsed && <span className="flex-none font-mono text-xs tabular-nums">{elapsed}</span>}
        <div className="absolute inset-x-0 bottom-0 h-[2px] overflow-hidden" aria-hidden>
          <div
            className="ct-stream-sweep h-full w-1/3"
            style={{ background: "var(--ct-accent, #58a6ff)" }}
          />
        </div>
      </div>
      <style>{`
        @keyframes ct-stream-sweep {
          from { transform: translateX(-100%); }
          to { transform: translateX(300%); }
        }
        @keyframes ct-stream-static-pulse {
          0%, 100% { opacity: 0.25; }
          50% { opacity: 0.8; }
        }
        .ct-stream-sweep { animation: ct-stream-sweep 1.2s linear infinite; }
        @media (prefers-reduced-motion: reduce) {
          .ct-stream-sweep {
            width: 100%;
            animation: ct-stream-static-pulse 1.6s ease-in-out infinite;
          }
        }
      `}</style>
    </div>
  );
}
