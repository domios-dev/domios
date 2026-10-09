import { useId, useState } from "react";
import { Timer, X } from "lucide-react";
import "./delayed-message-modal.css";

type SendMode = "in" | "at" | "every";
export type DelayedModalVariant = "create" | "edit" | "recurring";

export interface DelayedMessageModalProps {
  variant: DelayedModalVariant;
  text: string;
  minutes?: number;
}

const MINUTE_MS = 60_000;
const MODE_LABEL: Record<SendMode, string> = { in: "In", at: "At", every: "Every" };
const MODES: Record<DelayedModalVariant, SendMode[]> = { create: ["in", "at", "every"], edit: ["in", "at"], recurring: ["every"] };
const QUICK_PRESETS = [
  { label: "5m", minutes: 5 },
  { label: "10m", minutes: 10 },
  { label: "15m", minutes: 15 },
  { label: "30m", minutes: 30 },
  { label: "1h", minutes: 60 },
  { label: "3h", minutes: 180 },
  { label: "Tonight", at: { hour: 20, dayOffset: 0 } },
  { label: "Tomorrow", at: { hour: 9, dayOffset: 1 } },
] as const;

function atHour(nowMs: number, hour: number, dayOffset: number): number {
  const date = new Date(nowMs);
  date.setDate(date.getDate() + dayOffset);
  date.setHours(hour, 0, 0, 0);
  return date.getTime() > nowMs ? date.getTime() : atHour(nowMs, hour, dayOffset + 1);
}

function toLocalInputValue(ms: number): string {
  const date = new Date(ms);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

const formatClock = (ms: number) => new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" }).format(new Date(ms));

export function DelayedMessageModal({ variant, text, minutes: initialMinutes = 15 }: DelayedMessageModalProps) {
  const titleId = useId();
  const modes = MODES[variant];
  const [mode, setMode] = useState<SendMode>(modes[0]);
  const [minutes, setMinutes] = useState(initialMinutes);
  const [at, setAt] = useState(() => toLocalInputValue(Date.now() + initialMinutes * MINUTE_MS));
  const editing = variant !== "create";
  const scheduledAtMs = mode === "at" ? new Date(at).getTime() : Date.now() + minutes * MINUTE_MS;

  return <div className="ds-root ds-delayed-modal" role="dialog" aria-labelledby={titleId}>
    <div className="ds-delayed-modal__head">
      <Timer size={14} aria-hidden="true" className="ds-delayed-modal__icon" />
      <h2 id={titleId}>Delayed message</h2>
      <button type="button" className="ds-delayed-modal__close" aria-label="Close"><X size={14} aria-hidden="true" /></button>
    </div>
    <div className="ds-delayed-modal__body">
      <textarea aria-label="Message" defaultValue={text} />
      <div className="ds-delayed-modal__when">
        <span className="ds-delayed-modal__label">Send</span>
        <span className="ds-delayed-modal__seg" role="group" aria-label="When to send">
          {modes.map((option) => <button key={option} type="button" aria-pressed={mode === option} data-on={mode === option || undefined} onClick={() => setMode(option)}>{MODE_LABEL[option]}</button>)}
        </span>
        {mode === "at"
          ? <input aria-label="Send at" type="datetime-local" className="ds-delayed-modal__field" value={at} onChange={(event) => setAt(event.target.value)} />
          : <span className="ds-delayed-modal__field">
            <input aria-label={mode === "every" ? "Repeat every, minutes" : "Send in, minutes"} type="number" min={1} value={minutes} onChange={(event) => setMinutes(Math.max(1, Number(event.target.value) || 1))} />
            min
          </span>}
        {Number.isFinite(scheduledAtMs) && <span className="ds-delayed-modal__hint">{mode === "every" ? "next at" : "goes out at"} {formatClock(scheduledAtMs)}</span>}
      </div>
      {mode !== "every" && <div className="ds-delayed-modal__presets" role="group" aria-label="Quick times">
        {QUICK_PRESETS.map((preset) => <button key={preset.label} type="button" onClick={() => {
          if ("minutes" in preset) {
            setMode("in");
            setMinutes(preset.minutes);
          } else {
            setMode("at");
            setAt(toLocalInputValue(atHour(Date.now(), preset.at.hour, preset.at.dayOffset)));
          }
        }}>{preset.label}</button>)}
      </div>}
    </div>
    <div className="ds-delayed-modal__foot">
      {editing && <button type="button" className="ds-delayed-modal__btn ds-delayed-modal__btn--danger">Delete</button>}
      {editing && <button type="button" className="ds-delayed-modal__btn ds-delayed-modal__btn--quiet">Send now</button>}
      {!editing && <button type="button" className="ds-delayed-modal__btn ds-delayed-modal__btn--quiet">Stash</button>}
      <span className="ds-delayed-modal__spacer" />
      <button type="button" className="ds-delayed-modal__btn">Cancel</button>
      <button type="button" className="ds-delayed-modal__btn ds-delayed-modal__btn--primary">{editing ? "Save" : "Schedule"}</button>
    </div>
  </div>;
}
