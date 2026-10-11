import type { LarkCaptureState } from "./larkCaptureModel";

function statusMessage(state: LarkCaptureState): string {
  switch (state.phase) {
    case "starting": return "Starting hardware voice recording.";
    case "recording": return "Hardware voice recording is active. The clip stops at two minutes.";
    case "stopping": return "Stopping hardware voice recording and releasing the microphone.";
    case "transcribing": return "Transcribing the voice clip for its captured recipient.";
    case "error": return "Hardware voice recording failed.";
    case "disabled":
    case "armed": return "";
  }
}

export function LarkCaptureStatus({ state, onCancel }: {
  state: LarkCaptureState;
  onCancel: () => void;
}) {
  if (state.phase === "disabled" || state.phase === "armed") return null;
  return (
    <aside aria-label="Hardware voice recording" className="fixed bottom-4 right-4 z-50 flex max-w-sm flex-col gap-2 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)] p-3 shadow-lg">
      {state.phase !== "error" && (
        <p role="status" aria-live="polite" aria-atomic="true">{statusMessage(state)}</p>
      )}
      {state.error && <p role="alert" aria-live="assertive">{state.error}</p>}
      {state.capture && state.phase !== "error" && (
        <button type="button" aria-label="Cancel hardware voice recording" onClick={onCancel}>
          Cancel recording
        </button>
      )}
    </aside>
  );
}
