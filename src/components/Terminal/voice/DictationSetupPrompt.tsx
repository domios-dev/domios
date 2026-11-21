import { useRef, useState, type RefObject } from "react";
import { X } from "lucide-react";
import { AnchoredPopover } from "../../shared/AnchoredPopover";
import { Button } from "../../shared/Button";
import { askGeneralAgent, focusTab, generalAgentTab } from "../../../lib/generalAgent";
import { useMuxStateStore } from "../../../store/multiplexer/selectorStore";

let setupInFlight: Promise<void> | null = null;

export function clearDictationSetupHandoff() {
  setupInFlight = null;
}

export function DictationSetupPrompt({
  reason,
  onDismiss,
  anchor,
}: {
  reason: string;
  onDismiss(): void;
  anchor: RefObject<HTMLElement | null>;
}) {
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

  return (
    <AnchoredPopover open anchor={anchor} placement="top-end" gap={12} onClose={onDismiss}>
      <div
        className="ct-dictation-bubble ct-dictation-setup"
        data-testid="dictation-setup-bubble"
        role="group"
        aria-label="Install dictation"
      >
        <div className="ct-dictation-bubble__head">
          <strong>Install dictation?</strong>
          <button
            type="button"
            className="ct-dictation-bubble__close"
            aria-label="Dismiss dictation setup"
            title="Dismiss"
            disabled={pending}
            onClick={onDismiss}
          >
            <X size={14} strokeWidth={2.2} aria-hidden="true" />
          </button>
        </div>
        <p className="ct-dictation-bubble__body">{reason} General can guide the local model download.</p>
        <div className="ct-dictation-bubble__actions">
          <Button type="button" variant="primary" size="sm" loading={pending} disabled={pending} onClick={() => void install()}>
            Set up with General
          </Button>
        </div>
        {error && <div role="alert" className="ct-dictation-bubble__alert">{error}</div>}
      </div>
    </AnchoredPopover>
  );
}
