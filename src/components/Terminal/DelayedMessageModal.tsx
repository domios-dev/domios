import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Timer, X } from "lucide-react";
import { formatClock, parseLocalInputValue, toLocalInputValue, type DelayedMessage } from "./delayedSend";

type SendMode = "in" | "at" | "every";

export interface DelayedMessagePatch {
  text: string;
  scheduledAtMs: number;
  recurrence?: { everyMs: number };
}

function minutesUntil(ms: number): number {
  return Math.max(1, Math.round((ms - Date.now()) / 60_000));
}

export function DelayedMessageModal({ message, busy, onSave, onDelete, onClose }: {
  message: DelayedMessage;
  busy: boolean;
  onSave: (patch: DelayedMessagePatch) => void;
  onDelete: () => void;
  onClose: () => void;
}) {
  const titleId = useId();
  const textId = useId();
  const textRef = useRef<HTMLTextAreaElement>(null);
  const recurring = Boolean(message.recurrence);
  const [text, setText] = useState(message.text);
  const [mode, setMode] = useState<SendMode>(recurring ? "every" : "in");
  const [inMinutes, setInMinutes] = useState(String(minutesUntil(message.scheduledAtMs)));
  const [at, setAt] = useState(toLocalInputValue(message.scheduledAtMs));
  const [everyMinutes, setEveryMinutes] = useState(String((message.recurrence?.everyMs ?? 600_000) / 60_000));

  useEffect(() => {
    textRef.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  const minutes = Number(mode === "every" ? everyMinutes : inMinutes);
  const scheduledAtMs = mode === "at" ? parseLocalInputValue(at) : Number.isFinite(minutes) && minutes >= 1 ? Date.now() + minutes * 60_000 : null;
  const valid = text.trim().length > 0 && scheduledAtMs != null && scheduledAtMs > Date.now();
  const modes: SendMode[] = recurring ? ["every"] : ["in", "at"];
  const modeLabel: Record<SendMode, string> = { in: "In", at: "At", every: "Every" };

  const save = () => {
    if (!valid || scheduledAtMs == null) return;
    onSave({ text, scheduledAtMs, ...(mode === "every" ? { recurrence: { everyMs: minutes * 60_000 } } : {}) });
  };

  return createPortal(
    <div
      className="ct-delayed-modal__scrim"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div className="ct-delayed-modal" role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <div className="ct-delayed-modal__head">
          <Timer size={14} aria-hidden="true" className="ct-delayed-modal__icon" />
          <h2 id={titleId}>Delayed message</h2>
          <button type="button" className="ct-delayed-modal__close" aria-label="Close" disabled={busy} onClick={onClose}>
            <X size={14} aria-hidden="true" />
          </button>
        </div>
        <div className="ct-delayed-modal__body">
          <label className="sr-only" htmlFor={textId}>Message</label>
          <textarea
            id={textId}
            ref={textRef}
            value={text}
            onChange={(event) => setText(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                event.preventDefault();
                save();
              }
            }}
          />
          <div className="ct-delayed-modal__when">
            <span className="ct-delayed-modal__label">Send</span>
            <span className="ct-delayed-modal__seg" role="group" aria-label="When to send">
              {modes.map((option) => (
                <button key={option} type="button" aria-pressed={mode === option} data-on={mode === option || undefined} onClick={() => setMode(option)}>
                  {modeLabel[option]}
                </button>
              ))}
            </span>
            {mode === "at" ? (
              <input aria-label="Send at" type="datetime-local" className="ct-delayed-modal__field" value={at} onChange={(event) => setAt(event.target.value)} />
            ) : (
              <span className="ct-delayed-modal__field">
                <input
                  aria-label={mode === "every" ? "Repeat every, minutes" : "Send in, minutes"}
                  type="number"
                  min={1}
                  value={mode === "every" ? everyMinutes : inMinutes}
                  onChange={(event) => (mode === "every" ? setEveryMinutes : setInMinutes)(event.target.value)}
                />
                min
              </span>
            )}
            {scheduledAtMs != null && <span className="ct-delayed-modal__hint">{mode === "every" ? "next at" : "goes out at"} {formatClock(scheduledAtMs)}</span>}
          </div>
        </div>
        <div className="ct-delayed-modal__foot">
          <button type="button" className="ct-delayed-modal__btn ct-delayed-modal__btn--danger" disabled={busy} onClick={onDelete}>Delete</button>
          <span className="ct-delayed-modal__spacer" />
          <button type="button" className="ct-delayed-modal__btn" disabled={busy} onClick={onClose}>Cancel</button>
          <button type="button" className="ct-delayed-modal__btn ct-delayed-modal__btn--primary" disabled={busy || !valid} onClick={save}>Save</button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
