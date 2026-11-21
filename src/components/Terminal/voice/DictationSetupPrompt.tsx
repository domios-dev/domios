import { useRef, useState } from "react";
import { Button } from "../../shared/Button";
import { askGeneralAgent, focusTab, generalAgentTab } from "../../../lib/generalAgent";
import { useMuxStateStore } from "../../../store/multiplexer/selectorStore";

let setupInFlight: Promise<void> | null = null;

export function clearDictationSetupHandoff() {
  setupInFlight = null;
}

export function DictationSetupPrompt({ reason, onDismiss }: { reason: string; onDismiss(): void }) {
  const store = useMuxStateStore();
  const issued = useRef(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function install() {
    if (issued.current) return;
    issued.current = true;
    setPending(true);
    setError(null);
    try {
      const existing = setupInFlight;
      if (existing) {
        await existing;
        const general = generalAgentTab(store.getState());
        if (general) focusTab(general.tabId, general.workspaceId);
      } else {
        const request = askGeneralAgent(store.getState(),
          "Help me set up local live composer dictation in Domios. Use the official Transcriber plugin and its Russian-capable Parakeet default. Explain the model download size, permissions, progress, and any failure. Use the plugin's supported installation and configuration controls. Keep the existing voice-note transcription settings and my draft intact.")
          .catch(error => { if (setupInFlight === request) setupInFlight = null; throw error; });
        setupInFlight = request;
        await request;
      }
      onDismiss();
    } catch (failure) {
      issued.current = false;
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally { setPending(false); }
  }
  return <div className="ct-dictation-setup" role="group" aria-label="Install dictation">
    <div><strong>Install dictation?</strong><div>{reason} General can guide the local model download.</div></div>
    <div className="flex flex-wrap gap-1">
      <Button type="button" variant="primary" loading={pending} disabled={pending} onClick={() => void install()}>Set up with General</Button>
      <Button type="button" variant="ghost" disabled={pending} onClick={onDismiss}>Not now</Button>
    </div>
    {error && <div role="alert">{error}</div>}
  </div>;
}
