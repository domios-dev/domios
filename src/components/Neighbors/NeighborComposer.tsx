import { useRef, useState } from "react";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { ComposerActions } from "../Terminal/ComposerActions";
import { composerAttachmentHandoff } from "../Terminal/composerAttachmentHandoff";
import { beginComposition, createCompositionGate, endComposition, isImeKeyEvent } from "../Terminal/composerComposition";
import { classifyComposerDevice, composerEnterIntent } from "../Terminal/composerKeyboard";
import { ComposerTextInput } from "../Terminal/ComposerTextInput";
import { withTranscripts } from "../Surface/surfaceChromeModel";

/** The chat composer, sending text to a Neighbor: voice is transcribed into the message, files stay on this machine. */
export function NeighborComposer({ name, placeholder, disabled, sending, onSend, onError, transcribe = composerAttachmentHandoff.transcribeLocal }: {
  name: string; placeholder: string; disabled: boolean; sending: boolean;
  onSend: (body: string) => Promise<boolean>; onError: (message: string) => void;
  transcribe?: (path: string) => Promise<string>;
}) {
  const [draft, setDraft] = useState("");
  const [transcribing, setTranscribing] = useState(false);
  const latest = useRef(draft);
  latest.current = draft;
  const touchMode = useTouchOptimizedUi();
  const device = classifyComposerDevice({ userAgent: navigator.userAgent, platform: navigator.platform,
    maxTouchPoints: navigator.maxTouchPoints, touchOptimizedUi: touchMode });
  const gate = useRef(createCompositionGate()).current;
  const busy = sending || transcribing;
  const hasText = draft.trim().length > 0;

  const send = (text = latest.current) => {
    const body = text.trim();
    if (!body || busy || disabled) return;
    setDraft("");
    void onSend(body).then(ok => { if (!ok) setDraft(body); });
  };
  const applyVoice = async (paths: string[], andSend: boolean) => {
    setTranscribing(true);
    try {
      const message = withTranscripts(latest.current, await Promise.all(paths.map(path => transcribe(path))));
      if (andSend) send(message);
      else setDraft(message);
    } catch (failure: unknown) {
      onError(failure instanceof Error ? failure.message : "The recording could not be transcribed.");
    } finally { setTranscribing(false); }
  };

  return <div className="neighbor-composer">
    <ComposerActions canAttach={false} onAttach={paths => void applyVoice(paths, false)} onSend={() => send()}
      onSendWithAttachments={paths => void applyVoice(paths, true)} onError={onError}
      sendDisabled={disabled || busy || !hasText} hasText={hasText}>
      <ComposerTextInput aria-label={`Message to ${name}`} placeholder={placeholder} value={draft} readOnly={busy} disabled={disabled}
        enterKeyHint="send" onChange={event => setDraft(event.target.value)}
        onKeyDown={event => {
          if (isImeKeyEvent(gate, event.nativeEvent.isComposing, event.nativeEvent.keyCode)) return;
          if (composerEnterIntent(device, { code: event.code, key: event.key, keyCode: event.nativeEvent.keyCode, shiftKey: event.shiftKey,
            ctrlKey: event.ctrlKey, metaKey: event.metaKey, altKey: event.altKey, isComposing: event.nativeEvent.isComposing }) !== "submit") return;
          event.preventDefault();
          send();
        }}
        onCompositionStart={() => beginComposition(gate)} onCompositionEnd={() => endComposition(gate)} />
    </ComposerActions>
  </div>;
}
