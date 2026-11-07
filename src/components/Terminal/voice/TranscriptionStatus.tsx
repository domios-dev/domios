import { useCallback, useEffect, useRef, useState } from "react";

/** "m:ss" elapsed since a transcription started; no estimate, no percentage. */
export function formatTranscriptionElapsed(elapsedMs: number): string {
  const seconds = Math.max(0, Math.floor(elapsedMs / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/** Tracks sends that are waiting on voice transcription; `since` is the oldest one's start. */
export function useTranscriptionStatus(now: () => number = Date.now) {
  const active = useRef(new Map<number, number>());
  const nextId = useRef(0);
  const [since, setSince] = useState<number | null>(null);
  const track = useCallback(<T,>(work: Promise<T>): Promise<T> => {
    const id = nextId.current++;
    active.current.set(id, now());
    setSince(Math.min(...active.current.values()));
    const settle = () => {
      active.current.delete(id);
      setSince(active.current.size ? Math.min(...active.current.values()) : null);
    };
    return work.finally(settle);
  }, [now]);
  return { since, track };
}

export function TranscriptionStatus({ since, now = Date.now }: { since: number | null; now?: () => number }) {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (since === null) return;
    // A display clock only: it redraws the elapsed label and never gates delivery.
    const timer = setInterval(() => setTick(tick => tick + 1), 1000);
    return () => clearInterval(timer);
  }, [since]);
  if (since === null) return null;
  return <span role="status" aria-label="Voice transcription">Transcribing… {formatTranscriptionElapsed(now() - since)}</span>;
}
