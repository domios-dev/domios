import { useEffect, useState } from "react";

interface StreamPreviewFloatProps {
  rows: readonly string[];
  working: boolean;
  statusLine?: string | null;
}

const LINE_HEIGHT = 20;
const MAX_LINES = 2;
const BLOCK_HEIGHT = LINE_HEIGHT * MAX_LINES + 8;

function reducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * Bare streaming text, nothing else: the tail of the in-flight message
 * (matcher-cleaned), clamped to two lines, muted, in flow below the last
 * message only while the turn is working. Enters and exits by a 300ms
 * ease-out opacity+height animation (instant under prefers-reduced-motion);
 * reserves nothing when idle.
 */
export function StreamPreviewFloat({ rows, working, statusLine }: StreamPreviewFloatProps) {
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

  const tail = rows.length > 0 ? rows.slice(-MAX_LINES).join("\n") : statusLine ?? "";
  return (
    <div
      className="ct-stream-line w-full max-w-4xl min-w-0 mx-auto overflow-hidden"
      style={{ height: entered ? BLOCK_HEIGHT : 0, opacity: entered ? 1 : 0 }}
      onTransitionEnd={() => {
        if (!show) setMounted(false);
      }}
      data-testid="chat-stream-preview"
      data-float="line"
      role="status"
      aria-label="Streaming activity"
    >
      {/* Announced once via the canonical message when it lands; a live
          region here would re-announce every streamed change. */}
      <div
        className="line-clamp-2 whitespace-pre-wrap px-2.5 py-1 font-mono text-xs leading-5 opacity-80"
        style={{ color: "var(--ct-muted-fg)" }}
        data-testid="chat-stream-preview-line"
        aria-hidden
      >
        {tail}
      </div>
      <style>{`
        .ct-stream-line { transition: height 300ms ease-out, opacity 300ms ease-out; }
        @media (prefers-reduced-motion: reduce) {
          .ct-stream-line { transition: none; }
        }
      `}</style>
    </div>
  );
}
