import { useEffect, useRef, useState } from "react";
import { Pause, Play, SendHorizontal, Trash2 } from "lucide-react";
import type { VoicePreviewData } from "./useVoiceRecorder";

function formatClock(sec: number): string {
  const s = Math.max(0, Math.round(sec));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** Telegram-style recorded-clip preview: scrub the waveform, play it back,
 * then either trash or send. The played portion fills brighter as it plays. */
export function VoicePreview({
  data,
  busy,
  onDiscard,
  onSend,
}: {
  data: VoicePreviewData;
  busy: boolean;
  onDiscard: () => void;
  onSend: () => void;
}) {
  const audioRef = useRef<HTMLAudioElement>(null);
  const [playing, setPlaying] = useState(false);
  const [progress, setProgress] = useState(0);

  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;
    const onTime = () => {
      const dur = Number.isFinite(audio.duration) && audio.duration > 0 ? audio.duration : data.durationSec;
      setProgress(dur > 0 ? Math.min(1, audio.currentTime / dur) : 0);
    };
    const onEnd = () => {
      setPlaying(false);
      setProgress(0);
    };
    audio.addEventListener("timeupdate", onTime);
    audio.addEventListener("ended", onEnd);
    return () => {
      audio.removeEventListener("timeupdate", onTime);
      audio.removeEventListener("ended", onEnd);
    };
  }, [data.durationSec]);

  function toggle() {
    const audio = audioRef.current;
    if (!audio) return;
    if (playing) {
      audio.pause();
      setPlaying(false);
    } else {
      void audio.play().then(() => setPlaying(true)).catch(() => setPlaying(false));
    }
  }

  const remaining = playing || progress > 0 ? data.durationSec * (1 - progress) : data.durationSec;

  return (
    <div
      data-testid="composer-voice-preview"
      className="ct-voice-fade-in ct-voice-preview flex w-full items-center gap-2.5"
    >
      <audio ref={audioRef} src={data.url} preload="auto" className="hidden" />
      <button
        type="button"
        data-testid="composer-voice-preview-discard"
        title="Delete recording"
        aria-label="Delete recording"
        onClick={onDiscard}
        disabled={busy}
        className="ct-voice-control flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-[var(--ct-red,#f85149)] disabled:opacity-40"
      >
        <Trash2 size={18} strokeWidth={2.2} />
      </button>

      <button
        type="button"
        title={playing ? "Pause" : "Play"}
        aria-label={playing ? "Pause" : "Play"}
        onClick={toggle}
        className="ct-voice-control flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-white shadow-[0_3px_12px_rgba(31,111,235,0.4)]"
        style={{ background: "linear-gradient(145deg, #4f8cff 0%, #1f6feb 100%)" }}
      >
        {playing ? <Pause size={16} fill="currentColor" /> : <Play size={16} fill="currentColor" className="ml-0.5" />}
      </button>

      <div className="flex h-7 flex-1 items-center gap-[1.5px] overflow-hidden">
        {data.peaks.map((h, i) => {
          const played = i / data.peaks.length <= progress;
          return (
            <span
              key={i}
              className="w-[2px] shrink-0 rounded-[1px]"
              style={{
                height: `${Math.round(h * 100)}%`,
                background: played
                  ? "var(--ct-accent, #58a6ff)"
                  : "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 55%, transparent)",
                transition: "background var(--ct-duration-fast) var(--ct-ease-liquid-settle)",
              }}
            />
          );
        })}
      </div>

      <span className="shrink-0 font-mono text-[12px] tabular-nums text-[var(--ct-muted-fg)]">
        {formatClock(remaining)}
      </span>

      <button
        type="button"
        data-testid="composer-voice-preview-send"
        title="Send voice note"
        aria-label="Send voice note"
        onClick={onSend}
        disabled={busy}
        className="ct-voice-control flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-white shadow-[0_4px_16px_rgba(31,111,235,0.45)] disabled:opacity-50"
        style={{ background: "linear-gradient(145deg, #4f8cff 0%, #1f6feb 100%)" }}
      >
        <SendHorizontal size={17} fill="currentColor" />
      </button>
    </div>
  );
}
