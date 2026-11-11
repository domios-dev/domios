import { useState } from "react";
import { askGeneralAgent } from "../../../lib/generalAgent";
import { useMuxStateStore } from "../../../store/multiplexer/selectorStore";

export function isTranscriptionFailure(reason: string | null | undefined): boolean {
  return /^Voice transcription failed(?::|$)/.test(reason ?? "");
}

export function TranscriberSetupAction({ reason }: { reason: string }) {
  const store = useMuxStateStore();
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  async function setup() {
    setPending(true);
    setError(null);
    try {
      await askGeneralAgent(store.getState(), "Voice transcription failed. Help me set up the voice transcriber in Domios.");
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setPending(false);
    }
  }
  return <span role="status" aria-label={reason}>
    <span>{reason}</span>
    <button type="button" className="ct-delivery__retry" disabled={pending} onClick={() => void setup()}>Set up transcriber</button>
    {error && <span role="alert">{error}</span>}
  </span>;
}
