import { useMemo, useState } from "react";
import {
  CalendarClock,
  Check,
  Clock3,
  Infinity as InfinityIcon,
  Pencil,
  SendHorizontal,
  TriangleAlert,
  X,
} from "lucide-react";
import type { DelayedMessage } from "./delayedSend";
import {
  formatClock,
  formatRelative,
  parseLocalInputValue,
  QUICK_PRESETS,
  toLocalInputValue,
} from "./delayedSend";
import { FloatingTooltip } from "../shared/FloatingTooltip";
import { queuedMessagePreview } from "./queuedMessagePreview";
import { LiquidTabRail } from "../shared/LiquidTabRail";
import type { RecurrenceRule } from "./delayedSend";

const STASH_COLOR = "#a371f7";

const panelStyle = {
  color: "var(--ct-fg, #c9d1d9)",
};

const panelClass =
  "ct-panel-card mb-2 ml-2 w-[min(34rem,calc(100vw-1.5rem))] p-3 shadow-[0_16px_40px_rgba(0,0,0,0.38)] backdrop-blur-sm";

function PanelHeader({
  icon,
  title,
  onClose,
}: {
  icon: React.ReactNode;
  title: React.ReactNode;
  onClose: () => void;
}) {
  return (
    <div className="mb-2.5 flex items-center justify-between gap-2">
      <div className="flex min-w-0 items-center gap-2 text-[13px] font-semibold tracking-tight">
        <span style={{ color: "var(--ct-accent, #58a6ff)" }}>{icon}</span>
        <span>{title}</span>
      </div>
      <button
        type="button"
        data-testid="delayed-send-panel-close"
        aria-label="Close delayed messages panel"
        onMouseDown={(e) => e.preventDefault()}
        onClick={onClose}
        className="ct-liquid-interactive inline-flex h-7 w-7 items-center justify-center rounded-lg"
        style={{ color: "var(--ct-muted-fg)" }}
        title="Close"
      >
        <X size={15} />
      </button>
    </div>
  );
}

export function DelayedSendSchedulePanel({
  busy,
  touchMode,
  onClose,
  onConfirm,
  onStash,
}: {
  busy: boolean;
  touchMode: boolean;
  onClose: () => void;
  onConfirm: (scheduledAtMs: number, recurrence?: RecurrenceRule) => void;
  onStash: () => void;
}) {
  const [mode, setMode] = useState<"Send later" | "Repeat every">("Send later");
  const [value, setValue] = useState(() => toLocalInputValue(Date.now() + 5 * 60_000));
  const [customOpen, setCustomOpen] = useState(false);
  const [everyMs, setEveryMs] = useState(10 * 60_000);
  const [customEveryOpen, setCustomEveryOpen] = useState(false);
  const [customEveryMinutes, setCustomEveryMinutes] = useState("10");
  const [stopMode, setStopMode] = useState<"never" | "until" | "count">("never");
  const [untilValue, setUntilValue] = useState(() => toLocalInputValue(Date.now() + 24 * 60 * 60_000));
  const [countValue, setCountValue] = useState("3");
  const parsed = parseLocalInputValue(value);
  const parsedUntil = parseLocalInputValue(untilValue);
  const repeatCount = Number(countValue);
  const invalid = mode === "Send later"
    ? parsed == null || parsed <= Date.now() + 1_000
    : everyMs < 60_000
      || !Number.isInteger(Number(customEveryMinutes))
      || Number(customEveryMinutes) < 1
      || (stopMode === "until" && (parsedUntil == null || parsedUntil < Date.now() + everyMs))
      || (stopMode === "count" && (!Number.isInteger(repeatCount) || repeatCount < 1));

  const presets = useMemo(
    () => QUICK_PRESETS.map((p) => ({ label: p.label, ms: p.resolve(Date.now()) })),
    [value],
  );

  const activeLabel = useMemo(() => {
    if (mode === "Repeat every") return null;
    if (parsed == null) return null;
    const match = presets.find((p) => Math.abs(p.ms - parsed) < 30_000);
    return match?.label ?? null;
  }, [mode, parsed, presets]);

  const presetHeight = touchMode ? "h-11" : "h-9";
  const activeInterval = (ms: number) => everyMs === ms && !customEveryOpen;
  const intervalPresets = [
    { label: "5m", ms: 5 * 60_000 },
    { label: "10m", ms: 10 * 60_000 },
    { label: "30m", ms: 30 * 60_000 },
    { label: "1h", ms: 60 * 60_000 },
  ];

  return (
    <section
      role="dialog"
      aria-label="Schedule message"
      className={panelClass}
      style={panelStyle}
      data-testid="delayed-send-schedule-panel"
    >
      <PanelHeader
        icon={<CalendarClock size={15} />}
        title="Schedule message"
        onClose={onClose}
      />

      <LiquidTabRail
        value={mode}
        options={["Send later", "Repeat every"] as const}
        onChange={setMode}
        label="Schedule type"
        touch={touchMode}
        testIdPrefix="delayed-send-mode"
      />

      {mode === "Send later" ? (
        <div className="mt-2 grid grid-cols-4 gap-1.5">
          {presets.map((p) => {
            const active = activeLabel === p.label;
            return (
              <button
                key={p.label}
                type="button"
                data-testid="delayed-send-preset"
                data-preset-label={p.label}
                aria-pressed={active}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  const preset = QUICK_PRESETS.find((item) => item.label === p.label);
                  setValue(toLocalInputValue(preset?.resolve(Date.now()) ?? p.ms));
                  setCustomOpen(false);
                }}
                className={`ct-liquid-interactive ${presetHeight} rounded-lg border text-[13px] font-medium`}
                style={{
                  borderColor: active
                    ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 70%, transparent)"
                    : "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 20%, transparent)",
                  background: active
                    ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 22%, transparent)"
                    : "color-mix(in srgb, var(--ct-bg, #0d1117) 60%, white 4%)",
                  color: active ? "var(--ct-fg)" : "var(--ct-muted-fg)",
                }}
              >
                {p.label}
              </button>
            );
          })}
          <button
            type="button"
            data-testid="delayed-send-custom-toggle"
            aria-pressed={customOpen}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => setCustomOpen((v) => !v)}
            className={`ct-liquid-interactive ${presetHeight} inline-flex items-center justify-center gap-1 rounded-lg border text-[13px] font-medium`}
            style={{
              borderColor: customOpen
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 70%, transparent)"
                : "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 20%, transparent)",
              background: customOpen
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 22%, transparent)"
                : "color-mix(in srgb, var(--ct-bg, #0d1117) 60%, white 4%)",
              color: customOpen ? "var(--ct-fg)" : "var(--ct-muted-fg)",
            }}
          >
            <Pencil size={12} />
            <span>Custom</span>
          </button>
        </div>
      ) : (
        <div className="mt-2 grid grid-cols-4 gap-1.5">
          {intervalPresets.map((preset) => (
            <button
              key={preset.label}
              type="button"
              data-testid="delayed-send-interval-preset"
              data-preset-label={preset.label}
              aria-pressed={activeInterval(preset.ms)}
              onClick={() => {
                setEveryMs(preset.ms);
                setCustomEveryMinutes(String(preset.ms / 60_000));
                setCustomEveryOpen(false);
              }}
              className={`ct-liquid-interactive ${presetHeight} rounded-lg border text-[13px] font-medium`}
              style={{
                borderColor: activeInterval(preset.ms)
                  ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 70%, transparent)"
                  : "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 20%, transparent)",
                background: activeInterval(preset.ms)
                  ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 22%, transparent)"
                  : "color-mix(in srgb, var(--ct-bg, #0d1117) 60%, white 4%)",
                color: activeInterval(preset.ms) ? "var(--ct-fg)" : "var(--ct-muted-fg)",
              }}
            >
              {preset.label}
            </button>
          ))}
          <button
            type="button"
            data-testid="delayed-send-interval-custom-toggle"
            aria-pressed={customEveryOpen}
            onClick={() => setCustomEveryOpen((open) => !open)}
            className={`ct-liquid-interactive ${presetHeight} inline-flex items-center justify-center gap-1 rounded-lg border text-[13px] font-medium`}
            style={{
              borderColor: customEveryOpen
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 70%, transparent)"
                : "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 20%, transparent)",
              color: customEveryOpen ? "var(--ct-fg)" : "var(--ct-muted-fg)",
            }}
          >
            <Pencil size={12} />
            <span>Custom</span>
          </button>
          {customEveryOpen && (
            <label className="col-span-4 flex items-center gap-2 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
              Repeat every
              <input
                type="number"
                min={1}
                step={1}
                aria-label="Repeat interval in minutes"
                data-testid="delayed-send-custom-interval"
                value={customEveryMinutes}
                onChange={(event) => {
                  setCustomEveryMinutes(event.target.value);
                  const minutes = Number(event.target.value);
                  setEveryMs(Number.isFinite(minutes) ? minutes * 60_000 : 0);
                }}
                className={`w-20 rounded-lg border bg-transparent px-2 outline-none ${touchMode ? "h-11" : "h-9"}`}
                style={{ borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
              />
              minutes
            </label>
          )}
        </div>
      )}

      {mode === "Send later" && customOpen && (
        <input
          type="datetime-local"
          aria-label="Schedule time"
          data-testid="delayed-send-custom-input"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          className={`mt-2 w-full rounded-lg border bg-transparent px-2.5 text-[13px] outline-none ${touchMode ? "h-11" : "h-9"}`}
          style={{
            borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 26%, transparent)",
            color: "var(--ct-fg)",
            colorScheme: "dark",
          }}
        />
      )}

      {mode === "Repeat every" && (
        <div className="mt-3 grid gap-2">
          <label className="flex items-center gap-2 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
            Stop repeating
            <select
              aria-label="Stop repeating"
              data-testid="delayed-send-stop-mode"
              value={stopMode}
              onChange={(event) => setStopMode(event.target.value as typeof stopMode)}
              className="ml-auto rounded-lg border bg-transparent px-2 text-[13px]"
              style={{ borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
            >
              <option value="never">Never</option>
              <option value="until">At a time</option>
              <option value="count">After a number of ticks</option>
            </select>
          </label>
          {stopMode === "until" && (
            <label className="grid gap-1 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
              Stop at
              <input
                type="datetime-local"
                aria-label="Stop repeating at"
                data-testid="delayed-send-until"
                value={untilValue}
                onChange={(event) => setUntilValue(event.target.value)}
                className={`w-full rounded-lg border bg-transparent px-2.5 text-[13px] outline-none ${touchMode ? "h-11" : "h-9"}`}
                style={{ borderColor: "var(--ct-border)", color: "var(--ct-fg)", colorScheme: "dark" }}
              />
            </label>
          )}
          {stopMode === "count" && (
            <label className="flex items-center gap-2 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
              Stop after
              <input
                type="number"
                min={1}
                step={1}
                aria-label="Stop after tick count"
                data-testid="delayed-send-count"
                value={countValue}
                onChange={(event) => setCountValue(event.target.value)}
                className="w-20 rounded-lg border bg-transparent px-2 py-1 text-[13px]"
                style={{ borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
              />
              ticks
            </label>
          )}
        </div>
      )}

      <div className="mt-3 flex items-center gap-2">
        {mode === "Send later" && (
          <button
            type="button"
            data-testid="delayed-send-stash"
            onMouseDown={(e) => e.preventDefault()}
            onClick={onStash}
            disabled={busy}
            className={`ct-liquid-interactive inline-flex items-center justify-center gap-1.5 rounded-lg border px-3 text-[13px] font-medium disabled:opacity-40 ${touchMode ? "h-11" : "h-9"}`}
            style={{
              borderColor: `color-mix(in srgb, ${STASH_COLOR} 40%, transparent)`,
              background: `color-mix(in srgb, ${STASH_COLOR} 13%, transparent)`,
              color: STASH_COLOR,
            }}
            title="Stash indefinitely — never sends until you release it"
          >
            <InfinityIcon size={15} />
            <span>Stash</span>
          </button>
        )}

        <div className="ml-auto flex min-w-0 flex-col items-end leading-tight">
          {(mode === "Repeat every" || parsed != null) && !invalid ? (
            <>
              <span className="text-[13px] font-semibold tabular-nums">
                {formatRelative(mode === "Repeat every" ? Date.now() + everyMs : parsed ?? Date.now())}
              </span>
              <span className="text-[11px] font-mono" style={{ color: "var(--ct-muted-fg)" }}>
                {formatClock(mode === "Repeat every" ? Date.now() + everyMs : parsed ?? Date.now())}
              </span>
            </>
          ) : (
            <span className="text-[11px]" style={{ color: "var(--ct-red, #f85149)" }}>
              pick a future time
            </span>
          )}
        </div>

        <button
          type="button"
          data-testid="delayed-send-schedule-submit"
          onMouseDown={(e) => e.preventDefault()}
          className={`ct-liquid-interactive inline-flex items-center justify-center gap-1.5 rounded-lg px-4 text-[13px] font-semibold text-white shadow-sm disabled:opacity-40 ${touchMode ? "h-11" : "h-9"}`}
          disabled={busy || invalid}
          style={{ background: "linear-gradient(145deg, #4f8cff 0%, #1f6feb 100%)" }}
          onClick={() => {
            if (mode === "Send later" && parsed != null) {
              onConfirm(parsed);
              return;
            }
            if (mode === "Repeat every") {
              const nextAt = Date.now() + everyMs;
              onConfirm(nextAt, {
                everyMs,
                untilMs: stopMode === "until" ? parsedUntil : undefined,
                count: stopMode === "count" ? repeatCount : undefined,
              });
            }
          }}
        >
          <Check size={15} />
          <span>{mode === "Repeat every" ? "Repeat" : "Schedule"}</span>
        </button>
      </div>
    </section>
  );
}

function statusMeta(msg: DelayedMessage, nowMs: number) {
  if (msg.state === "failed") {
    return {
      color: "var(--ct-red, #f85149)",
      icon: <TriangleAlert size={14} />,
      label: msg.error ?? "failed",
    };
  }
  if (msg.state === "stashed") {
    return { color: STASH_COLOR, icon: <InfinityIcon size={14} />, label: "Stashed" };
  }
  return {
    color: "var(--ct-accent, #58a6ff)",
    icon: <Clock3 size={14} />,
    label: formatRelative(msg.scheduledAtMs, nowMs),
  };
}

function CardAction({
  icon,
  title,
  color,
  disabled,
  touchMode,
  onClick,
}: {
  icon: React.ReactNode;
  title: string;
  color?: string;
  disabled?: boolean;
  touchMode: boolean;
  onClick: () => void;
}) {
  const size = touchMode ? "h-9 w-9" : "h-7 w-7";
  return (
    <button
      type="button"
      data-testid="delayed-send-card-action"
      data-action-title={title}
      aria-label={title}
      onMouseDown={(e) => e.preventDefault()}
      disabled={disabled}
      onClick={onClick}
      title={title}
      className={`ct-liquid-interactive inline-flex ${size} items-center justify-center rounded-lg active:scale-90 disabled:opacity-30`}
      style={{ color: color ?? "var(--ct-muted-fg)" }}
    >
      {icon}
    </button>
  );
}

function intervalLabel(ms: number): string {
  if (ms % 3_600_000 === 0) return `${ms / 3_600_000}h`;
  if (ms % 60_000 === 0) return `${ms / 60_000}m`;
  return `${Math.round(ms / 1_000)}s`;
}

function QueueCard({
  msg,
  nowMs,
  busy,
  touchMode,
  editing,
  text,
  time,
  everyMinutes,
  onText,
  onTime,
  onEveryMinutes,
  onStartEdit,
  onSaveEdit,
  onCancelEdit,
  onSendNow,
  onDelete,
}: {
  msg: DelayedMessage;
  nowMs: number;
  busy: boolean;
  touchMode: boolean;
  editing: boolean;
  text: string;
  time: string;
  everyMinutes: string;
  onText: (v: string) => void;
  onTime: (v: string) => void;
  onEveryMinutes: (v: string) => void;
  onStartEdit: () => void;
  onSaveEdit: () => void;
  onCancelEdit: () => void;
  onSendNow: () => void;
  onDelete: () => void;
}) {
  const meta = statusMeta(msg, nowMs);
  const failed = msg.state === "failed";
  const preview = useMemo(() => queuedMessagePreview(msg.text), [msg.text]);

  return (
    <div
      data-testid="delayed-send-queue-item"
      data-delayed-message-id={msg.id}
      data-delayed-message-state={msg.state}
      className="group min-w-0 rounded-xl border px-2.5 py-2 transition-colors"
      style={{
        borderColor: failed
          ? "color-mix(in srgb, var(--ct-red, #f85149) 30%, transparent)"
          : "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 14%, transparent)",
        background: "color-mix(in srgb, var(--ct-bg, #0d1117) 50%, white 4%)",
      }}
    >
      {editing ? (
        <div className="grid gap-2">
          <textarea
            data-testid="delayed-send-edit-text"
            aria-label="Reminder message"
            value={text}
            onChange={(e) => onText(e.target.value)}
            className="min-h-16 resize-y rounded-lg border bg-transparent px-2.5 py-1.5 text-[13px] leading-5 outline-none"
            style={{
              borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 26%, transparent)",
              color: "var(--ct-fg)",
            }}
          />
          {msg.recurrence && (
            <label className="flex items-center gap-2 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
              Repeat every
              <input
                type="number"
                min={1}
                step={1}
                aria-label="Repeat interval in minutes"
                data-testid="delayed-send-edit-interval"
                value={everyMinutes}
                onChange={(event) => onEveryMinutes(event.target.value)}
                className="w-20 rounded-lg border bg-transparent px-2 py-1 text-[13px]"
                style={{ borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
              />
              minutes
            </label>
          )}
          <div className="flex flex-wrap items-center gap-2">
            <input
              type="datetime-local"
              data-testid="delayed-send-edit-time"
              aria-label="Next fire time"
              value={time}
              onChange={(e) => onTime(e.target.value)}
              className={`min-w-[12rem] flex-1 rounded-lg border bg-transparent px-2.5 text-[13px] outline-none ${touchMode ? "h-10" : "h-8"}`}
              style={{
                borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 26%, transparent)",
                color: "var(--ct-fg)",
                colorScheme: "dark",
              }}
            />
            <CardAction
              icon={<Check size={16} />}
              title="Save"
              color="var(--ct-green, #3fb950)"
              touchMode={touchMode}
              onClick={onSaveEdit}
            />
            <CardAction
              icon={<X size={16} />}
              title="Cancel"
              touchMode={touchMode}
              onClick={onCancelEdit}
            />
          </div>
        </div>
      ) : (
        <div className="flex min-w-0 items-start gap-2">
          <span className="mt-0.5 shrink-0" style={{ color: meta.color }}>
            {meta.icon}
          </span>
          <span className="min-w-0 flex-1">
            <FloatingTooltip
              content={preview}
              touchMode={touchMode}
              className="line-clamp-2 min-w-0 cursor-default break-words text-[13px] leading-5 outline-none"
              style={{
                color: "var(--ct-fg)",
                display: "-webkit-box",
                WebkitBoxOrient: "vertical",
                WebkitLineClamp: 2,
                overflow: "hidden",
                overflowWrap: "anywhere",
                whiteSpace: "pre-wrap",
              }}
              tooltipClassName="max-h-none overflow-hidden"
              tooltipStyle={{ maxHeight: "12rem", overflow: "hidden" }}
            >
              {preview}
            </FloatingTooltip>
            {msg.recurrence && (
              <span className="mt-0.5 block truncate text-[10px]" style={{ color: "var(--ct-muted-fg)" }}>
                repeats every {intervalLabel(msg.recurrence.everyMs)} · next at {formatClock(msg.scheduledAtMs)} · skipped {msg.skippedTicks ?? 0}
              </span>
            )}
          </span>
          {msg.sessionChanged && (
            <span style={{ color: "var(--ct-yellow, #d29922)" }} title="Session changed since scheduled">
              <TriangleAlert size={13} />
            </span>
          )}
          <span
            className="shrink-0 max-w-[8rem] truncate text-[11px] font-medium"
            style={{ color: meta.color }}
          >
            {meta.label}
          </span>
          <div className="flex shrink-0 items-center gap-0.5">
            <CardAction
              icon={<Pencil size={15} />}
              title="Edit"
              disabled={busy}
              touchMode={touchMode}
              onClick={onStartEdit}
            />
            <CardAction
              icon={<SendHorizontal size={15} />}
              title="Send now"
              color="var(--ct-green, #3fb950)"
              disabled={busy}
              touchMode={touchMode}
              onClick={onSendNow}
            />
            <CardAction
              icon={<X size={15} />}
              title={msg.recurrence ? "Stop recurring series" : "Delete"}
              color="var(--ct-red, #f85149)"
              disabled={busy}
              touchMode={touchMode}
              onClick={onDelete}
            />
          </div>
        </div>
      )}
    </div>
  );
}

export function DelayedSendQueuePanel({
  messages,
  busyId,
  touchMode,
  onClose,
  onDelete,
  onSendNow,
  onUpdate,
}: {
  messages: DelayedMessage[];
  busyId: string | null;
  touchMode: boolean;
  onClose: () => void;
  onDelete: (id: string) => void;
  onSendNow: (id: string) => void;
  onUpdate: (
    id: string,
    patch: { text?: string; scheduledAtMs?: number; stash?: boolean; recurrence?: RecurrenceRule },
  ) => void;
}) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [time, setTime] = useState("");
  const [everyMinutes, setEveryMinutes] = useState("10");
  const nowMs = Date.now();

  function startEdit(msg: DelayedMessage) {
    setEditingId(msg.id);
    setText(msg.text);
    const base = msg.state === "stashed" ? Date.now() + 15 * 60_000 : msg.scheduledAtMs;
    setTime(toLocalInputValue(base));
    setEveryMinutes(String((msg.recurrence?.everyMs ?? 600_000) / 60_000));
  }

  function saveEdit(id: string) {
    const scheduledAtMs = parseLocalInputValue(time);
    if (scheduledAtMs == null) return;
    const message = messages.find((item) => item.id === id);
    const minutes = Number(everyMinutes);
    if (message?.recurrence && (!Number.isInteger(minutes) || minutes < 1)) return;
    onUpdate(id, {
      text,
      scheduledAtMs,
      ...(message?.recurrence
        ? { recurrence: { ...message.recurrence, everyMs: minutes * 60_000 } }
        : {}),
    });
    setEditingId(null);
  }

  return (
    <div className={panelClass} style={panelStyle} data-testid="delayed-send-queue-panel">
      <PanelHeader
        icon={<Clock3 size={15} />}
        title={`${messages.length} queued`}
        onClose={onClose}
      />
      <div className="grid max-h-72 min-w-0 gap-2 overflow-y-auto overflow-x-hidden pr-0.5">
        {messages.length === 0 ? (
          <div
            data-testid="delayed-send-queue-empty"
            className="rounded-xl border border-dashed px-3 py-6 text-center text-xs"
            style={{
              color: "var(--ct-muted-fg)",
              borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
            }}
          >
            Nothing queued
          </div>
        ) : (
          messages.map((msg) => (
            <QueueCard
              key={msg.id}
              msg={msg}
              nowMs={nowMs}
              busy={busyId === msg.id || msg.state === "sending"}
              touchMode={touchMode}
              editing={editingId === msg.id}
              text={text}
              time={time}
              everyMinutes={everyMinutes}
              onText={setText}
              onTime={setTime}
              onEveryMinutes={setEveryMinutes}
              onStartEdit={() => startEdit(msg)}
              onSaveEdit={() => saveEdit(msg.id)}
              onCancelEdit={() => setEditingId(null)}
              onSendNow={() => onSendNow(msg.id)}
              onDelete={() => onDelete(msg.id)}
            />
          ))
        )}
      </div>
    </div>
  );
}
