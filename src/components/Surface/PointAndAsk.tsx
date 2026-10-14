import { Heading, Image, Link, MousePointerClick, TextCursorInput, Type, type LucideIcon } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { classifyComposerDevice, composerEnterIntent } from "../Terminal/composerKeyboard";
import { beginComposition, createCompositionGate, endComposition, isImeKeyEvent } from "../Terminal/composerComposition";
import { trapDialogTab } from "../shared/dialogFocusTrap";
import { AttachmentChip } from "../Terminal/ComposerBubbles";
import { ComposerActions } from "../Terminal/ComposerActions";
import { composerAttachmentHandoff } from "../Terminal/composerAttachmentHandoff";
import { measureComposerAutosize } from "../Terminal/composerAutosize";
import { askDialogLabel, askPlaceholder, SNIP_DIALOG_LABEL, snipPlaceholder, withTranscripts, type ElementCard, type ElementKind } from "./surfaceChromeModel";

/** What the question is about: a picked element, or a snipped area whose image chip can be removed. */
export type AskSubject =
  | { kind: "element"; card: ElementCard }
  | { kind: "area"; context: string; image: { thumbnail: string | null } | null; onRemoveImage(): void };

export const SNIP_IMAGE_LABEL = "screenshot";

export interface PointAndAskProps {
  owner: string;
  subject: AskSubject;
  position: { left: number; top: number };
  sending?: boolean;
  onSend(comment: string): void;
  onCancel(): void;
  onError?(message: string): void;
  transcribe?(path: string): Promise<string>;
}

const KIND_ICON: Record<ElementKind, LucideIcon> = {
  link: Link,
  button: MousePointerClick,
  image: Image,
  heading: Heading,
  input: TextCursorInput,
  text: Type,
};

const ASK_EDITOR_MAX_PX = 80;
const STAGE = ".ct-surface-stage";
const STAGE_CANVAS = ".ct-surface-canvas";

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

function SubjectHeader({ subject, describedId }: { subject: AskSubject; describedId: string }) {
  if (subject.kind === "area") {
    return (
      <>
        {subject.image && <AttachmentChip path={SNIP_IMAGE_LABEL} label={SNIP_IMAGE_LABEL} thumbnail={subject.image.thumbnail} onRemove={subject.onRemoveImage} />}
        <span id={describedId} className="ct-surface-ask__context" title={subject.context}>{subject.context}</span>
      </>
    );
  }
  const { card } = subject;
  const Icon = KIND_ICON[card.kind];
  return (
    <span className="ct-surface-ask__chip" title={card.path || undefined} data-kind={card.kind}>
      <Icon size={12} strokeWidth={2.2} aria-hidden="true" />
      <span className="ct-surface-ask__chip-name">{card.name}</span>
    </span>
  );
}

export function PointAndAsk({
  owner,
  subject,
  position,
  sending = false,
  onSend,
  onCancel,
  onError = () => undefined,
  transcribe = composerAttachmentHandoff.transcribeLocal,
}: PointAndAskProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const [draft, setDraft] = useState("");
  const [transcribing, setTranscribing] = useState(false);
  const pathId = useId();
  const touchMode = useTouchOptimizedUi();
  const composerDevice = classifyComposerDevice({
    userAgent: navigator.userAgent, platform: navigator.platform,
    maxTouchPoints: navigator.maxTouchPoints, touchOptimizedUi: touchMode,
  });
  const compositionGate = useRef(createCompositionGate()).current;
  const busy = sending || transcribing;
  const hasText = draft.trim().length > 0;
  const latest = useRef({ draft, onCancel });
  latest.current = { draft, onCancel };
  const description = subject.kind === "element" ? subject.card.path : subject.context;

  useLayoutEffect(() => {
    if (editorRef.current) measureComposerAutosize(editorRef.current, null, ASK_EDITOR_MAX_PX);
  }, [draft]);

  useEffect(() => {
    const onDocumentClick = (event: MouseEvent) => {
      const dialog = dialogRef.current;
      const target = event.target as Element | null;
      if (!dialog || !target || dialog.contains(target)) return;
      // A stage click would otherwise re-pick or report the watch-only notice.
      if (target.closest?.(STAGE_CANVAS) && dialog.closest(STAGE)?.contains(target)) {
        event.preventDefault();
        event.stopPropagation();
      }
      if (latest.current.draft.trim()) editorRef.current?.focus({ preventScroll: true });
      else latest.current.onCancel();
    };
    document.addEventListener("click", onDocumentClick, true);
    return () => document.removeEventListener("click", onDocumentClick, true);
  }, []);

  const submit = () => {
    const comment = draft.trim();
    if (!busy && comment) onSend(comment);
  };

  const applyVoice = async (paths: string[], send: boolean) => {
    setTranscribing(true);
    try {
      const transcripts = await Promise.all(paths.map((path) => transcribe(path)));
      const comment = withTranscripts(latest.current.draft, transcripts);
      if (!send) setDraft(comment);
      else if (comment) onSend(comment);
    } catch (error) {
      onError(errorText(error));
    } finally {
      setTranscribing(false);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape" && !event.shiftKey) {
      event.preventDefault();
      event.stopPropagation();
      onCancel();
    } else {
      trapDialogTab(event, dialogRef.current);
    }
  };

  return (
    <div
      ref={dialogRef}
      role="dialog"
      aria-label={subject.kind === "element" ? askDialogLabel(subject.card) : SNIP_DIALOG_LABEL}
      aria-describedby={description ? pathId : undefined}
      aria-busy={busy || undefined}
      data-sending={busy ? "true" : undefined}
      className="ct-surface-ask"
      style={{ left: position.left, top: position.top }}
      onKeyDown={onKeyDown}
    >
      {subject.kind === "element" && description && <span id={pathId} hidden>{description}</span>}
      <ComposerActions
        canAttach={false}
        onAttach={(paths) => void applyVoice(paths, false)}
        onSend={submit}
        onSendWithAttachments={(paths) => void applyVoice(paths, true)}
        onError={onError}
        sendDisabled={busy || !hasText}
        hasText={hasText}
        cardHeader={<SubjectHeader subject={subject} describedId={pathId} />}
      >
        <textarea
          ref={editorRef}
          autoFocus
          rows={1}
          aria-label="Your question"
          placeholder={subject.kind === "element" ? askPlaceholder(owner) : snipPlaceholder(owner)}
          value={draft}
          readOnly={busy}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (isImeKeyEvent(compositionGate, event.nativeEvent.isComposing, event.nativeEvent.keyCode)) return;
            if (composerEnterIntent(composerDevice, {
              code: event.code, key: event.key, keyCode: event.nativeEvent.keyCode,
              shiftKey: event.shiftKey, ctrlKey: event.ctrlKey, metaKey: event.metaKey,
              altKey: event.altKey, isComposing: event.nativeEvent.isComposing,
            }) !== "submit") return;
            event.preventDefault();
            submit();
          }}
          onCompositionStart={() => beginComposition(compositionGate)}
          onCompositionEnd={() => endComposition(compositionGate)}
          autoComplete="off"
          spellCheck={false}
          enterKeyHint="send"
          className="ct-surface-ask__editor block w-full min-h-5 resize-none bg-transparent py-0 pr-1 text-[14px] leading-5 outline-none placeholder:text-[var(--ct-muted-fg)]"
        />
      </ComposerActions>
    </div>
  );
}
