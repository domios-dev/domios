import { useMemo, useState } from "react";
import { Check, Clock3, Pencil, SendHorizontal, Trash2, X } from "lucide-react";
import type { DelayedMessage } from "./delayedSend";
import {
  formatScheduledTime,
  parseLocalInputValue,
  toLocalInputValue,
} from "./delayedSend";

const panelStyle = {
  background: "color-mix(in srgb, var(--ct-bg, #0d1117) 94%, black 6%)",
  borderColor:
    "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 24%, transparent)",
  color: "var(--ct-fg, #c9d1d9)",
};

const iconButton =
  "inline-flex h-7 w-7 items-center justify-center rounded-md text-xs transition hover:brightness-125 disabled:opacity-40";

function nextDefaultValue(): string {
  return toLocalInputValue(Date.now() + 5 * 60_000);
}

export function DelayedSendSchedulePanel({
  busy,
  onClose,
  onConfirm,
}: {
  busy: boolean;
  onClose: () => void;
  onConfirm: (scheduledAtMs: number) => void;
}) {
  const [value, setValue] = useState(nextDefaultValue);
  const parsed = parseLocalInputValue(value);
  const invalid = parsed == null || parsed <= Date.now() + 1_000;
  const quick = useMemo(
    () => [
      { label: "5m", ms: 5 * 60_000 },
      { label: "15m", ms: 15 * 60_000 },
      { label: "1h", ms: 60 * 60_000 },
    ],
    [],
  );
  return (
    <div
      className="mb-2 ml-2 w-[min(32rem,calc(100vw-3rem))] rounded-md border p-2 shadow-[0_10px_28px_rgba(0,0,0,0.24)]"
      style={panelStyle}
    >
      <div className="mb-2 flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2 text-xs font-medium">
          <Clock3 size={13} />
          <span>Schedule send</span>
        </div>
        <button
          type="button"
          className={iconButton}
          style={{ color: "var(--ct-muted-fg)" }}
          onClick={onClose}
          title="Close"
        >
          <X size={14} />
        </button>
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        {quick.map((q) => (
          <button
            key={q.label}
            type="button"
            className="h-8 rounded border px-2 text-xs font-mono hover:brightness-125"
            style={{
              borderColor:
                "color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)",
              background:
                "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, transparent)",
            }}
            onClick={() => setValue(toLocalInputValue(Date.now() + q.ms))}
          >
            {q.label}
          </button>
        ))}
        <input
          type="datetime-local"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          className="h-8 min-w-[12rem] flex-1 rounded border bg-transparent px-2 text-xs outline-none"
          style={{
            borderColor:
              "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 28%, transparent)",
            color: "var(--ct-fg)",
          }}
        />
        <button
          type="button"
          className="inline-flex h-8 items-center gap-1 rounded border px-2 text-xs font-medium hover:brightness-125 disabled:opacity-40"
          disabled={busy || invalid}
          style={{
            borderColor: "rgba(255,255,255,0.18)",
            background: "linear-gradient(145deg, #4f8cff 0%, #1f6feb 100%)",
            color: "white",
          }}
          onClick={() => {
            if (parsed != null) onConfirm(parsed);
          }}
        >
          <Check size={13} />
          <span>Apply</span>
        </button>
      </div>
    </div>
  );
}

export function DelayedSendQueuePanel({
  messages,
  busyId,
  onClose,
  onDelete,
  onSendNow,
  onUpdate,
}: {
  messages: DelayedMessage[];
  busyId: string | null;
  onClose: () => void;
  onDelete: (id: string) => void;
  onSendNow: (id: string) => void;
  onUpdate: (id: string, patch: { text?: string; scheduledAtMs?: number }) => void;
}) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [time, setTime] = useState("");

  function startEdit(msg: DelayedMessage) {
    setEditingId(msg.id);
    setText(msg.text);
    setTime(toLocalInputValue(msg.scheduledAtMs));
  }

  function saveEdit(id: string) {
    const scheduledAtMs = parseLocalInputValue(time);
    if (scheduledAtMs == null) return;
    onUpdate(id, { text, scheduledAtMs });
    setEditingId(null);
  }

  return (
    <div
      className="mb-2 ml-2 w-[min(32rem,calc(100vw-3rem))] rounded-md border px-2 py-1.5 shadow-[0_10px_28px_rgba(0,0,0,0.24)]"
      style={panelStyle}
    >
      <div className="mb-2 flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2 text-xs font-medium">
          <Clock3 size={13} />
          <span>{messages.length} queued</span>
        </div>
        <button
          type="button"
          className={iconButton}
          style={{ color: "var(--ct-muted-fg)" }}
          onClick={onClose}
          title="Close"
        >
          <X size={14} />
        </button>
      </div>
      <div className="grid max-h-64 overflow-y-auto">
        {messages.length === 0 ? (
          <div
            className="px-1 py-3 text-center text-xs"
            style={{ color: "var(--ct-muted-fg)" }}
          >
            Empty
          </div>
        ) : (
          messages.map((msg) => {
            const editing = editingId === msg.id;
            const busy = busyId === msg.id || msg.state === "sending";
            return (
              <div
                key={msg.id}
                className="border-t px-1 py-1.5 first:border-t-0"
                style={{
                  borderColor:
                    msg.state === "failed"
                      ? "color-mix(in srgb, var(--ct-red, #f85149) 38%, transparent)"
                      : "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 18%, transparent)",
                }}
              >
                {editing ? (
                  <div className="grid gap-1.5">
                    <textarea
                      value={text}
                      onChange={(e) => setText(e.target.value)}
                      className="min-h-14 resize-y rounded border bg-transparent px-2 py-1 text-xs outline-none"
                      style={{
                        borderColor:
                          "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 28%, transparent)",
                        color: "var(--ct-fg)",
                      }}
                    />
                    <div className="flex flex-wrap items-center gap-1.5">
                      <input
                        type="datetime-local"
                        value={time}
                        onChange={(e) => setTime(e.target.value)}
                        className="h-8 min-w-[12rem] flex-1 rounded border bg-transparent px-2 text-xs outline-none"
                        style={{
                          borderColor:
                            "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 28%, transparent)",
                          color: "var(--ct-fg)",
                        }}
                      />
                      <button
                        type="button"
                        className={iconButton}
                        style={{
                          background:
                            "linear-gradient(145deg, #4f8cff 0%, #1f6feb 100%)",
                          color: "white",
                        }}
                        onClick={() => saveEdit(msg.id)}
                        title="Save"
                      >
                        <Check size={14} />
                      </button>
                      <button
                        type="button"
                        className={iconButton}
                        style={{ color: "var(--ct-muted-fg)" }}
                        onClick={() => setEditingId(null)}
                        title="Cancel"
                      >
                        <X size={14} />
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2">
                    <div className="min-w-0">
                      <div className="line-clamp-2 whitespace-pre-wrap text-xs leading-5">
                        {msg.text}
                      </div>
                      <div
                        className="mt-0.5 flex flex-wrap items-center gap-1.5 text-[11px] font-mono"
                        style={{ color: "var(--ct-muted-fg)" }}
                      >
                        <span>{formatScheduledTime(msg.scheduledAtMs)}</span>
                        {msg.sessionChanged && (
                          <span style={{ color: "var(--ct-yellow, #d29922)" }}>
                            session changed
                          </span>
                        )}
                        {msg.state === "failed" && (
                          <span style={{ color: "var(--ct-red, #f85149)" }}>
                            {msg.error ?? "failed"}
                          </span>
                        )}
                      </div>
                    </div>
                    <div className="flex items-center gap-0.5">
                      <button
                        type="button"
                        className={iconButton}
                        disabled={busy}
                        style={{ color: "var(--ct-muted-fg)" }}
                        onClick={() => startEdit(msg)}
                        title="Edit"
                      >
                        <Pencil size={13} />
                      </button>
                      <button
                        type="button"
                        className={iconButton}
                        disabled={busy}
                        style={{ color: "var(--ct-muted-fg)" }}
                        onClick={() => onSendNow(msg.id)}
                        title="Send now"
                      >
                        <SendHorizontal size={13} />
                      </button>
                      <button
                        type="button"
                        className={iconButton}
                        disabled={busy}
                        style={{ color: "var(--ct-red, #f85149)" }}
                        onClick={() => onDelete(msg.id)}
                        title="Delete"
                      >
                        <Trash2 size={13} />
                      </button>
                    </div>
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
