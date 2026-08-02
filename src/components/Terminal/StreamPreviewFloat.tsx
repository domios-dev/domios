import { useEffect, useState } from "react";

interface StreamPreviewFloatProps {
  rows: readonly string[];
}

const LINE_HEIGHT = 20;
const MAX_LINES = 2;
const BLOCK_HEIGHT = LINE_HEIGHT * MAX_LINES + 8;

function reducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function StreamPreviewFloat({ rows }: StreamPreviewFloatProps) {
  const show = rows.length > 0;
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

  const tail = rows.slice(-MAX_LINES).join("\n");
  return (
    <div
      className="ct-stream-line w-full max-w-4xl min-w-0 mx-auto overflow-hidden"
      style={{
        height: entered ? BLOCK_HEIGHT : 0,
        opacity: entered ? 1 : 0,
        transform: entered ? "translateY(0)" : "translateY(-4px)",
      }}
      onTransitionEnd={(event) => {
        if (!show && event.propertyName === "height") setMounted(false);
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
        .ct-stream-line {
          transition: height 420ms var(--ct-ease-liquid-settle), opacity 360ms ease-out, transform 420ms var(--ct-ease-liquid-settle);
        }
        @media (prefers-reduced-motion: reduce) {
          .ct-stream-line { transition: none; }
        }
      `}</style>
    </div>
  );
}
