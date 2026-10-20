import { InlineErrorNotice } from "../../shared/ErrorNotice";
import type { LocalRecordingCaptureState } from "./localRecordingCaptureModel";

function statusMessage(state: LocalRecordingCaptureState): string {
  switch (state.phase) {
    case "starting": return "Starting hardware voice recording.";
    case "recording": return "Hardware voice recording is active.";
    case "stopping": return "Stopping hardware voice recording and releasing the microphone.";
    case "transcribing": return "Transcribing the voice clip for its captured recipient.";
    case "error": return "Hardware voice recording failed.";
    case "disabled":
    case "armed": return "";
  }
}

export function LocalRecordingCaptureStatus({ state }: {
  state: LocalRecordingCaptureState;
}) {
  if (state.phase === "disabled" || state.phase === "armed") return null;
  return (
    <div className="sr-only">
      {state.phase !== "error" && (
        <p role="status" aria-live="polite" aria-atomic="true">{statusMessage(state)}</p>
      )}
      {state.error && <InlineErrorNotice error={state.error} operation="recording this voice note" blocking />}
    </div>
  );
}
