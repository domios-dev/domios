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

const LINE_HEIGHT = 28;

function reducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * Plain extra STATUS LINE (no box, no surface — same visual family as the
 * bottom agent-status line) shown in flow below the last message only while
 * the turn is working: pulsing dot, Working, the latest truncated preview
 * tail, right-aligned elapsed. Enters and exits by a 300ms ease-out
 * opacity+height animation (instant under prefers-reduced-motion); reserves
 * nothing when idle. One line, never grows.
 */
export function StreamPreviewFloat({ rows, working, statusLine, elapsedMs }: StreamPreviewFloatProps) {
  const show = working || rows.length > 0;
  const [mounted, setMounted] = useState(show);
  const [entered, setEntered] = useState(show);
  useEffect(() => {
    if (show) {
      setMounted(true);
      return;
    }
    setEntered(false);
    if (reducedMotion()) setMounted(false);
  }, [show]);
  useEffect(() => {
    if (!mounted || !show) return;
    if (reducedMotion()) {
      setEntered(true);
      return;
    }
    // Commit one frame at height 0 / opacity 0 so the enter transition runs.
    const raf = requestAnimationFrame(() => setEntered(true));
    return () => cancelAnimationFrame(raf);
  }, [mounted, show]);
  if (!mounted) return null;

  const lastLine = rows.length > 0 ? rows[rows.length - 1] : statusLine ?? "";
  const elapsed = formatElapsed(elapsedMs);
  return (
    <div
      className="ct-stream-line w-full max-w-4xl min-w-0 mx-auto overflow-hidden"
      style={{ height: entered ? LINE_HEIGHT : 0, opacity: entered ? 1 : 0 }}
      onTransitionEnd={() => {
        if (!show) setMounted(false);
      }}
      data-testid="chat-stream-preview"
      data-float="line"
      role="status"
      aria-label="Streaming activity"
    >
      <div
        className="flex items-center gap-2 px-2.5"
        style={{ height: LINE_HEIGHT, color: "var(--ct-muted-fg)" }}
      >
        <span
          className="ct-stream-dot inline-block h-2 w-2 flex-none animate-pulse rounded-full"
          style={{ background: "var(--ct-accent, #58a6ff)" }}
        />
        <span className="flex-none font-mono text-xs">Working</span>
        {/* Announced once via the canonical message when it lands; a live
            region here would re-announce every streamed line change. */}
        <span
          className="min-w-0 flex-1 truncate whitespace-nowrap font-mono text-xs opacity-80"
          data-testid="chat-stream-preview-line"
          aria-hidden
        >
          {lastLine}
        </span>
        {elapsed && <span className="flex-none font-mono text-xs tabular-nums">{elapsed}</span>}
      </div>
      <style>{`
        .ct-stream-line { transition: height 300ms ease-out, opacity 300ms ease-out; }
        @media (prefers-reduced-motion: reduce) {
          .ct-stream-line { transition: none; }
          .ct-stream-dot { animation: none; }
        }
      `}</style>
    </div>
  );
}
