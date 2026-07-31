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

/**
 * Fixed-height ghost panel pinned at the bottom of the chat column, OUT of
 * document flow (zero-height anchor + absolute overlay): the settled chat
 * above never reflows while a turn streams. The box never resizes; it dies
 * by opacity only. Header (pulsing dot + action + elapsed) is always on top;
 * the body is a tail-anchored viewport whose old lines dissolve upward
 * through a fade mask instead of hard-clipping.
 */
export function StreamPreviewFloat({ rows, working, statusLine, elapsedMs }: StreamPreviewFloatProps) {
  const show = working || rows.length > 0;
  const [mounted, setMounted] = useState(show);
  useEffect(() => {
    if (show) setMounted(true);
  }, [show]);
  if (!mounted) return null;

  const elapsed = formatElapsed(elapsedMs);
  return (
    <div className="relative z-20 h-0">
      <div
        className="pointer-events-none absolute inset-x-0 bottom-1 mx-auto w-full max-w-4xl px-2 transition-opacity duration-200"
        style={{ opacity: show ? 1 : 0 }}
        onTransitionEnd={() => {
          if (!show) setMounted(false);
        }}
        data-testid="chat-stream-preview"
        data-float="fixed"
        role="status"
        aria-label="Streaming preview"
      >
        <div
          className="flex flex-col overflow-hidden rounded-md px-2.5 py-1.5 shadow-lg"
          style={{
            height: "9.5rem",
            background: "color-mix(in srgb, var(--ct-surface, #161b22) 72%, transparent)",
            backdropFilter: "blur(10px)",
            WebkitBackdropFilter: "blur(10px)",
            borderLeft: "2px solid var(--ct-accent, #58a6ff)",
          }}
        >
          <div
            className="flex flex-none items-center gap-2 font-mono text-xs"
            data-testid="chat-stream-preview-status"
            style={{ color: "var(--ct-muted-fg)" }}
          >
            <span
              className="inline-block h-2 w-2 flex-none animate-pulse rounded-full"
              style={{ background: "var(--ct-accent, #58a6ff)" }}
            />
            <span className="min-w-0 flex-1 truncate">{statusLine || "Working"}</span>
            {elapsed && <span className="flex-none tabular-nums">{elapsed}</span>}
          </div>
          <div
            className="flex min-h-0 flex-1 flex-col justify-end overflow-hidden"
            style={{
              maskImage: "linear-gradient(to bottom, transparent 0, black 2rem)",
              WebkitMaskImage: "linear-gradient(to bottom, transparent 0, black 2rem)",
            }}
          >
            <pre
              className="whitespace-pre-wrap font-mono text-xs opacity-80"
              style={{ color: "var(--ct-muted-fg)" }}
            >
              {rows.join("\n")}
            </pre>
          </div>
        </div>
      </div>
    </div>
  );
}
