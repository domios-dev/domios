import { ActivityDots } from "../../shared/ActivityDots";
import { useReducedMotion } from "../../Canvas/missionControl/useReducedMotion";

export function VoiceTranscriptionProgress() {
  const reducedMotion = useReducedMotion();
  return <span role="status" aria-label="Voice transcription" aria-busy="true"
    data-motion={reducedMotion ? "reduced" : "animated"}
    className="inline-flex items-baseline gap-2 text-[var(--ct-muted-fg)]">
    <ActivityDots size="small" tone="muted" rhythm="streaming" active={!reducedMotion} />
    <span>Transcribing voice note…</span>
  </span>;
}
