import { useRef, useState } from "react";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { ComposerActions } from "../Terminal/ComposerActions";
import { composerAttachmentHandoff } from "../Terminal/composerAttachmentHandoff";
import { beginComposition, createCompositionGate, endComposition, isImeKeyEvent } from "../Terminal/composerComposition";
import { classifyComposerDevice, composerEnterIntent } from "../Terminal/composerKeyboard";
import { ComposerTextInput } from "../Terminal/ComposerTextInput";
import { withTranscripts } from "../Surface/surfaceChromeModel";
import { ComposerBubbleRow } from "../Terminal/ComposerBubbles";
import { isVoiceNote } from "./neighborFilesClient";

const MAX_FILES = 4;

/** The chat composer, sending to a Neighbor: voice is transcribed into the message, and files go along when the conversation allows them. */
export function NeighborComposer({ name, placeholder, disabled, sending, onSend, onSendFiles, onError, transcribe = composerAttachmentHandoff.transcribeLocal }: {
  name: string; placeholder: string; disabled: boolean; sending: boolean;
  onSend: (body: string) => Promise<boolean>; onError: (message: string) => void;
  /** Present when files can go along; without it the paperclip stays away. */
  onSendFiles?: (body: string, paths: string[]) => Promise<boolean>;
  transcribe?: (path: string) => Promise<string>;
}) {
  const [draft, setDraft] = useState("");
  const [transcribing, setTranscribing] = useState(false);
  const [files, setFiles] = useState<string[]>([]);
  const latest = useRef(draft);
  latest.current = draft;
  const touchMode = useTouchOptimizedUi();
  const device = classifyComposerDevice({ userAgent: navigator.userAgent, platform: navigator.platform,
    maxTouchPoints: navigator.maxTouchPoints, touchOptimizedUi: touchMode });
  const gate = useRef(createCompositionGate()).current;
  const busy = sending || transcribing;
  const hasText = draft.trim().length > 0 || files.length > 0;

  const send = (text = latest.current) => {
    const body = text.trim();
    const attached = files;
    if ((!body && attached.length === 0) || busy || disabled) return;
    setDraft("");
    setFiles([]);
    const sent = attached.length > 0 && onSendFiles ? onSendFiles(body, attached) : onSend(body);
    void sent.then(ok => { if (!ok) { setDraft(body); setFiles(attached); } });
  };
  // Recorded voice notes become words; anything else picked or dropped rides along as a file.
  const attach = (paths: string[]) => {
    const voice = paths.filter(isVoiceNote);
    const picked = paths.filter(path => !isVoiceNote(path));
    if (voice.length > 0) void applyVoice(voice, false);
    if (picked.length === 0) return;
    const next = [...files, ...picked.filter(path => !files.includes(path))];
    if (next.length > MAX_FILES) onError(`Up to ${MAX_FILES} files per message.`);
    setFiles(next.slice(0, MAX_FILES));
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
    <ComposerActions canAttach={Boolean(onSendFiles) && !disabled} onAttach={paths => onSendFiles ? attach(paths) : void applyVoice(paths, false)} onSend={() => send()}
      onSendWithAttachments={paths => void applyVoice(paths, true)} onError={onError}
      sendDisabled={disabled || busy || !hasText} hasText={hasText}
      cardHeader={files.length > 0 && <ComposerBubbleRow attachments={files} onRemove={path => setFiles(current => current.filter(entry => entry !== path))} />}>
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
