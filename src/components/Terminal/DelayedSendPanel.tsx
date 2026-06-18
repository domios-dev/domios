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

const STASH_COLOR = "#a371f7";

const panelStyle = {
  background: "color-mix(in srgb, var(--ct-bg, #0d1117) 92%, black 8%)",
  borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
  color: "var(--ct-fg, #c9d1d9)",
};

const panelClass =
  "mb-2 ml-2 w-[min(34rem,calc(100vw-1.5rem))] rounded-xl border p-3 shadow-[0_16px_40px_rgba(0,0,0,0.38)] backdrop-blur-sm";

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
        onMouseDown={(e) => e.preventDefault()}
        onClick={onClose}
        className="inline-flex h-7 w-7 items-center justify-center rounded-lg transition hover:bg-[color-mix(in_srgb,var(--ct-muted-fg,#8b949e)_18%,transparent)]"
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
  onConfirm: (scheduledAtMs: number) => void;
  onStash: () => void;
}) {
  const [value, setValue] = useState(() => toLocalInputValue(Date.now() + 5 * 60_000));
  const [customOpen, setCustomOpen] = useState(false);
  const parsed = parseLocalInputValue(value);
  const invalid = parsed == null || parsed <= Date.now() + 1_000;

  const presets = useMemo(() => {
    const now = Date.now();
    return QUICK_PRESETS.map((p) => ({ label: p.label, ms: p.resolve(now) }));
  }, []);

  const activeLabel = useMemo(() => {
    if (parsed == null) return null;
    const match = presets.find((p) => Math.abs(p.ms - parsed) < 30_000);
    return match?.label ?? null;
  }, [parsed, presets]);

  const presetHeight = touchMode ? "h-11" : "h-9";

  return (
    <div className={panelClass} style={panelStyle} data-testid="delayed-send-schedule-panel">
      <PanelHeader
        icon={<CalendarClock size={15} />}
        title="Schedule message"
        onClose={onClose}
      />

      <div className="grid grid-cols-4 gap-1.5 sm:grid-cols-5">
        {presets.map((p) => {
          const active = activeLabel === p.label;
          return (
            <button
              key={p.label}
              type="button"
              data-testid="delayed-send-preset"
              data-preset-label={p.label}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                setValue(toLocalInputValue(p.ms));
                setCustomOpen(false);
              }}
              className={`${presetHeight} rounded-lg border text-[13px] font-medium transition active:scale-[0.97]`}
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
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => setCustomOpen((v) => !v)}
          className={`${presetHeight} inline-flex items-center justify-center gap-1 rounded-lg border text-[13px] font-medium transition active:scale-[0.97]`}
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

      {customOpen && (
        <input
          type="datetime-local"
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

      <div className="mt-3 flex items-center gap-2">
        <button
          type="button"
          data-testid="delayed-send-stash"
          onMouseDown={(e) => e.preventDefault()}
          onClick={onStash}
          disabled={busy}
          className={`inline-flex items-center justify-center gap-1.5 rounded-lg border px-3 text-[13px] font-medium transition active:scale-[0.98] disabled:opacity-40 ${touchMode ? "h-11" : "h-9"}`}
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

        <div className="ml-auto flex min-w-0 flex-col items-end leading-tight">
          {parsed != null && !invalid ? (
            <>
              <span className="text-[13px] font-semibold tabular-nums">
                {formatRelative(parsed)}
              </span>
              <span className="text-[11px] font-mono" style={{ color: "var(--ct-muted-fg)" }}>
                {formatClock(parsed)}
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
          className={`inline-flex items-center justify-center gap-1.5 rounded-lg px-4 text-[13px] font-semibold text-white shadow-sm transition active:scale-[0.98] disabled:opacity-40 ${touchMode ? "h-11" : "h-9"}`}
          disabled={busy || invalid}
          style={{ background: "linear-gradient(145deg, #4f8cff 0%, #1f6feb 100%)" }}
          onClick={() => {
            if (parsed != null) onConfirm(parsed);
          }}
        >
          <Check size={15} />
          <span>Schedule</span>
        </button>
      </div>
    </div>
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
      onMouseDown={(e) => e.preventDefault()}
      disabled={disabled}
      onClick={onClick}
      title={title}
      className={`inline-flex ${size} items-center justify-center rounded-lg transition hover:bg-[color-mix(in_srgb,var(--ct-muted-fg,#8b949e)_16%,transparent)] active:scale-90 disabled:opacity-30`}
      style={{ color: color ?? "var(--ct-muted-fg)" }}
    >
      {icon}
    </button>
  );
}

function QueueCard({
  msg,
  nowMs,
  busy,
  touchMode,
  editing,
  text,
  time,
  onText,
  onTime,
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
  onText: (v: string) => void;
  onTime: (v: string) => void;
  onStartEdit: () => void;
  onSaveEdit: () => void;
  onCancelEdit: () => void;
  onSendNow: () => void;
  onDelete: () => void;
}) {
  const meta = statusMeta(msg, nowMs);
  const failed = msg.state === "failed";

  return (
    <div
      data-testid="delayed-send-queue-item"
      data-delayed-message-id={msg.id}
      data-delayed-message-state={msg.state}
      className="group rounded-xl border px-2.5 py-2 transition-colors"
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
            value={text}
            onChange={(e) => onText(e.target.value)}
            className="min-h-16 resize-y rounded-lg border bg-transparent px-2.5 py-1.5 text-[13px] leading-5 outline-none"
            style={{
              borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 26%, transparent)",
              color: "var(--ct-fg)",
            }}
          />
          <div className="flex flex-wrap items-center gap-2">
            <input
              type="datetime-local"
              data-testid="delayed-send-edit-time"
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
        <div className="flex min-w-0 items-center gap-2">
          <span className="shrink-0" style={{ color: meta.color }}>
            {meta.icon}
          </span>
          <FloatingTooltip
            content={msg.text}
            touchMode={touchMode}
            className="min-w-0 flex-1 cursor-default truncate text-[13px] outline-none"
            style={{ color: "var(--ct-fg)" }}
          >
            {msg.text}
          </FloatingTooltip>
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
              title="Delete"
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
    patch: { text?: string; scheduledAtMs?: number; stash?: boolean },
  ) => void;
}) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [time, setTime] = useState("");
  const nowMs = Date.now();

  function startEdit(msg: DelayedMessage) {
    setEditingId(msg.id);
    setText(msg.text);
    const base = msg.state === "stashed" ? Date.now() + 15 * 60_000 : msg.scheduledAtMs;
    setTime(toLocalInputValue(base));
  }

  function saveEdit(id: string) {
    const scheduledAtMs = parseLocalInputValue(time);
    if (scheduledAtMs == null) return;
    onUpdate(id, { text, scheduledAtMs });
    setEditingId(null);
  }

  return (
    <div className={panelClass} style={panelStyle} data-testid="delayed-send-queue-panel">
      <PanelHeader
        icon={<Clock3 size={15} />}
        title={`${messages.length} queued`}
        onClose={onClose}
      />
      <div className="grid max-h-72 gap-2 overflow-y-auto pr-0.5">
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
              onText={setText}
              onTime={setTime}
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
