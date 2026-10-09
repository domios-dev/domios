import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Timer, X } from "lucide-react";
import {
  formatClock,
  parseLocalInputValue,
  QUICK_PRESETS,
  toLocalInputValue,
  type DelayedMessage,
  type RecurrenceRule,
} from "./delayedSend";

type SendMode = "in" | "at" | "every";

export type DelayedModalTarget =
  | { kind: "create"; text: string }
  | { kind: "edit"; message: DelayedMessage };

export interface DelayedMessagePatch {
  text: string;
  scheduledAtMs: number;
  recurrence?: RecurrenceRule;
}

const STASH_DEFAULT_MS = 15 * 60_000;
const MODE_LABEL: Record<SendMode, string> = { in: "In", at: "At", every: "Every" };

function minutesUntil(ms: number): number {
  return Math.max(1, Math.round((ms - Date.now()) / 60_000));
}

function initialTime(target: DelayedModalTarget): number {
  if (target.kind === "create" || target.message.state === "stashed") return Date.now() + STASH_DEFAULT_MS;
  return target.message.scheduledAtMs;
}

export function DelayedMessageModal({ target, busy, onSubmit, onStash, onDelete, onSendNow, onClose }: {
  target: DelayedModalTarget;
  busy: boolean;
  onSubmit: (patch: DelayedMessagePatch) => void;
  onStash?: (text: string) => void;
  onDelete?: () => void;
  onSendNow?: () => void;
  onClose: () => void;
}) {
  const titleId = useId();
  const textId = useId();
  const textRef = useRef<HTMLTextAreaElement>(null);
  const editing = target.kind === "edit" ? target.message : null;
  const recurrence = editing?.recurrence ?? null;
  const [text, setText] = useState(editing ? editing.text : target.kind === "create" ? target.text : "");
  const [mode, setMode] = useState<SendMode>(recurrence ? "every" : "in");
  const [inMinutes, setInMinutes] = useState(String(minutesUntil(initialTime(target))));
  const [at, setAt] = useState(toLocalInputValue(initialTime(target)));
  const [everyMinutes, setEveryMinutes] = useState(String((recurrence?.everyMs ?? 600_000) / 60_000));

  useEffect(() => {
    textRef.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  const modes: SendMode[] = recurrence ? ["every"] : editing ? ["in", "at"] : ["in", "at", "every"];
  const minutes = Number(mode === "every" ? everyMinutes : inMinutes);
  const minutesValid = Number.isInteger(minutes) && minutes >= 1;
  const scheduledAtMs = mode === "at" ? parseLocalInputValue(at) : minutesValid ? Date.now() + minutes * 60_000 : null;
  const valid = text.trim().length > 0 && scheduledAtMs != null && scheduledAtMs > Date.now();

  const submit = () => {
    if (!valid || scheduledAtMs == null) return;
    onSubmit({
      text,
      scheduledAtMs,
      ...(mode === "every" ? { recurrence: { ...recurrence, everyMs: minutes * 60_000 } } : {}),
    });
  };

  const applyPreset = (resolve: (nowMs: number) => number, label: string) => {
    const ms = resolve(Date.now());
    if (/^\d+[mh]$/.test(label)) {
      setMode("in");
      setInMinutes(String(minutesUntil(ms)));
    } else {
      setMode("at");
      setAt(toLocalInputValue(ms));
    }
  };

  return createPortal(
    <div
      className="ct-delayed-modal__scrim"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div className="ct-delayed-modal" role="dialog" aria-modal="true" aria-labelledby={titleId} data-testid="delayed-message-modal">
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
                submit();
              }
            }}
          />
          <div className="ct-delayed-modal__when">
            <span className="ct-delayed-modal__label">Send</span>
            <span className="ct-delayed-modal__seg" role="group" aria-label="When to send">
              {modes.map((option) => (
                <button key={option} type="button" aria-pressed={mode === option} data-on={mode === option || undefined} onClick={() => setMode(option)}>
                  {MODE_LABEL[option]}
                </button>
              ))}
            </span>
            {mode === "at" ? (
              <input aria-label="Send at" type="datetime-local" className="ct-delayed-modal__field" value={at} onChange={(event) => setAt(event.target.value)} />
            ) : (
              <span className="ct-delayed-modal__field">
                <input
                  data-testid="delayed-minutes"
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
          {mode !== "every" && (
            <div className="ct-delayed-modal__presets" role="group" aria-label="Quick times">
              {QUICK_PRESETS.map((preset) => (
                <button key={preset.label} type="button" onClick={() => applyPreset(preset.resolve, preset.label)}>{preset.label}</button>
              ))}
            </div>
          )}
        </div>
        <div className="ct-delayed-modal__foot">
          {onDelete && <button type="button" className="ct-delayed-modal__btn ct-delayed-modal__btn--danger" disabled={busy} onClick={onDelete}>Delete</button>}
          {onSendNow && <button type="button" className="ct-delayed-modal__btn ct-delayed-modal__btn--quiet" disabled={busy} onClick={onSendNow}>Send now</button>}
          {onStash && <button type="button" className="ct-delayed-modal__btn ct-delayed-modal__btn--quiet" disabled={busy || !text.trim()} onClick={() => onStash(text)}>Stash</button>}
          <span className="ct-delayed-modal__spacer" />
          <button type="button" className="ct-delayed-modal__btn" disabled={busy} onClick={onClose}>Cancel</button>
          <button type="button" className="ct-delayed-modal__btn ct-delayed-modal__btn--primary" disabled={busy || !valid} onClick={submit}>
            {editing ? "Save" : "Schedule"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
