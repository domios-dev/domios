import { useEffect, useRef, useState } from "react";
import { ChevronLeft } from "lucide-react";

function formatPrecise(ms: number): string {
  const totalCenti = Math.floor(ms / 10);
  const cs = totalCenti % 100;
  const totalSec = Math.floor(totalCenti / 100);
  const s = totalSec % 60;
  const m = Math.floor(totalSec / 60);
  return `${m}:${String(s).padStart(2, "0")},${String(cs).padStart(2, "0")}`;
}

/** In-composer recording indicator. Replaces the text input while a voice
 * note is being captured: pulsing red dot + live timer, plus a slide-to-cancel
 * hint (hold mode) or an explicit Cancel button (hands-free locked mode). */
export function VoiceRecordingBar({
  mode,
  armed = false,
  cancelHintRef,
  onCancel,
}: {
  mode: "hold" | "locked";
  armed?: boolean;
  cancelHintRef: React.RefObject<HTMLDivElement | null>;
  onCancel: () => void;
}) {
  const [ms, setMs] = useState(0);
  const startRef = useRef(performance.now());

  useEffect(() => {
    startRef.current = performance.now();
    const t = window.setInterval(() => setMs(performance.now() - startRef.current), 50);
    return () => window.clearInterval(t);
  }, []);

  return (
    <div className="ct-voice-fade-in flex w-full items-center gap-2">
      <span
        className="ct-voice-rec-dot h-2.5 w-2.5 shrink-0 rounded-full"
        style={{ background: "var(--ct-red, #f85149)" }}
      />
      <span className="shrink-0 font-mono text-[13px] tabular-nums text-[var(--ct-fg)]">
        {formatPrecise(ms)}
      </span>

      {mode === "hold" ? (
        <div
          ref={cancelHintRef}
          data-testid="composer-voice-cancel-hint"
          role="button"
          tabIndex={0}
          onPointerDown={(e) => {
            e.preventDefault();
            onCancel();
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              onCancel();
            }
          }}
          className={`ml-auto flex items-center gap-1 pr-1 text-[12px] font-medium transition-colors ${
            armed
              ? "scale-105 text-[var(--ct-red,#f85149)]"
              : "ct-voice-cancel-slide text-[var(--ct-muted-fg)]"
          }`}
        >
          <ChevronLeft size={14} />
          <span>{armed ? "Release to cancel" : "Slide to cancel"}</span>
        </div>
      ) : (
        <button
          ref={cancelHintRef as unknown as React.RefObject<HTMLButtonElement>}
          type="button"
          data-testid="composer-voice-cancel-hint"
          onPointerDown={(e) => {
            e.preventDefault();
            onCancel();
          }}
          className="ml-auto rounded-full px-3 py-1 text-[12px] font-semibold uppercase tracking-wide text-[var(--ct-red,#f85149)] transition-colors hover:bg-[color-mix(in_srgb,var(--ct-red,#f85149)_14%,transparent)]"
        >
          Cancel
        </button>
      )}
    </div>
  );
}
