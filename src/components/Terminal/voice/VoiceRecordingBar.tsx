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
  peaks,
  cancelHintRef,
  onCancel,
}: {
  mode: "hold" | "locked";
  armed?: boolean;
  peaks: number[];
  cancelHintRef: React.RefObject<HTMLDivElement | null>;
  onCancel: () => void;
}) {
  const [ms, setMs] = useState(0);
  const startRef = useRef(performance.now());
  const visiblePeaks = peaks.length > 0 ? peaks : [0.1];

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

      <div
        data-testid="composer-voice-live-waveform"
        className="mx-1 grid h-8 min-w-0 flex-1 items-center gap-[2px] overflow-hidden rounded-full px-2 py-1"
        style={{
          gridTemplateColumns: `repeat(${visiblePeaks.length}, minmax(2px, 1fr))`,
          background: "color-mix(in srgb, var(--ct-bg,#0d1117) 32%, transparent)",
        }}
        aria-hidden
      >
        {visiblePeaks.map((peak, index) => (
          <span
            key={index}
            className="w-full rounded-full"
            style={{
              height: `${Math.max(12, Math.round(peak * 100))}%`,
              background: armed
                ? "color-mix(in srgb, var(--ct-red,#f85149) 82%, white 8%)"
                : "color-mix(in srgb, var(--ct-accent,#58a6ff) 72%, var(--ct-muted-fg,#8b949e) 28%)",
              opacity: 0.42 + Math.min(0.58, peak),
              transition: "height 80ms linear, opacity 80ms linear, background 120ms linear",
            }}
          />
        ))}
      </div>

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
          className={`flex shrink-0 items-center gap-1 pr-1 text-[12px] font-medium transition-colors ${
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
          className="shrink-0 rounded-full px-3 py-1 text-[12px] font-semibold uppercase tracking-wide text-[var(--ct-red,#f85149)] transition-colors hover:bg-[color-mix(in_srgb,var(--ct-red,#f85149)_14%,transparent)]"
        >
          Cancel
        </button>
      )}
    </div>
  );
}
