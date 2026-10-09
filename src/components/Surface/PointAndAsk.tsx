import { Heading, Image, Link, MousePointerClick, TextCursorInput, Type, type LucideIcon } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { trapDialogTab } from "../shared/dialogFocusTrap";
import { ComposerActions } from "../Terminal/ComposerActions";
import { composerAttachmentHandoff } from "../Terminal/composerAttachmentHandoff";
import { measureComposerAutosize } from "../Terminal/composerAutosize";
import { askDialogLabel, askPlaceholder, withTranscripts, type ElementCard, type ElementKind } from "./surfaceChromeModel";

export interface PointAndAskProps {
  owner: string;
  card: ElementCard;
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

export function PointAndAsk({
  owner,
  card,
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
  const busy = sending || transcribing;
  const hasText = draft.trim().length > 0;
  const latest = useRef({ draft, onCancel });
  latest.current = { draft, onCancel };
  const Icon = KIND_ICON[card.kind];

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
      aria-label={askDialogLabel(card)}
      aria-describedby={card.path ? pathId : undefined}
      aria-busy={busy || undefined}
      data-sending={busy ? "true" : undefined}
      className="ct-surface-ask"
      style={{ left: position.left, top: position.top }}
      onKeyDown={onKeyDown}
    >
      {card.path && <span id={pathId} hidden>{card.path}</span>}
      <ComposerActions
        canAttach={false}
        onAttach={(paths) => void applyVoice(paths, false)}
        onSend={submit}
        onSendWithAttachments={(paths) => void applyVoice(paths, true)}
        onError={onError}
        sendDisabled={busy || !hasText}
        hasText={hasText}
        cardHeader={
          <span className="ct-surface-ask__chip" title={card.path || undefined} data-kind={card.kind}>
            <Icon size={12} strokeWidth={2.2} aria-hidden="true" />
            <span className="ct-surface-ask__chip-name">{card.name}</span>
          </span>
        }
      >
        <textarea
          ref={editorRef}
          autoFocus
          rows={1}
          aria-label="Your question"
          placeholder={askPlaceholder(owner)}
          value={draft}
          readOnly={busy}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing) return;
            event.preventDefault();
            submit();
          }}
          autoComplete="off"
          spellCheck={false}
          enterKeyHint="send"
          className="ct-surface-ask__editor block w-full min-h-5 resize-none bg-transparent py-0 pr-1 text-[14px] leading-5 outline-none placeholder:text-[var(--ct-muted-fg)]"
        />
      </ComposerActions>
    </div>
  );
}
