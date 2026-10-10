import React from "react";
import { AlarmClock, X } from "lucide-react";
import { renderUserContent } from "../MarkdownBody";
import { absoluteTime, relativeTime } from "./relativeTime";

interface Props {
  whoSet?: string;
  model?: string;
  setAt?: string;
  fireAt?: string;
  id?: string;
  everyMs?: string;
  nextAt?: string;
  skippedTicks?: string;
  onStop?: (id: string) => void;
  body: string;
}

function intervalLabel(value: string | undefined): string | null {
  const ms = Number(value);
  if (!Number.isFinite(ms) || ms < 1) return null;
  if (ms % 3_600_000 === 0) return `${ms / 3_600_000}h`;
  if (ms % 60_000 === 0) return `${ms / 60_000}m`;
  return `${Math.round(ms / 1_000)}s`;
}

function clockLabel(value: string | undefined): string | null {
  const ms = Number(value);
  if (!Number.isFinite(ms)) return null;
  return new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" }).format(new Date(ms));
}

export function ReminderCard({
  whoSet,
  model,
  setAt,
  fireAt,
  id,
  everyMs,
  nextAt,
  skippedTicks,
  onStop,
  body,
}: Props): React.ReactElement {
  const setRel = relativeTime(setAt);
  const fireRel = relativeTime(fireAt);
  const every = intervalLabel(everyMs);
  const next = clockLabel(nextAt);
  const repeatLine = every
    ? `repeats every ${every}${next ? ` · next at ${next}` : ""} · skipped ${skippedTicks ?? "0"}`
    : null;
  const subline = [setRel ? `set ${setRel}` : null, fireRel ? `fired ${fireRel}` : null]
    .filter(Boolean)
    .join(" · ");
  const sublineTitle = [
    setAt ? `set ${absoluteTime(setAt)}` : null,
    fireAt ? `fired ${absoluteTime(fireAt)}` : null,
  ]
    .filter(Boolean)
    .join("\n");
  return (
    <div className="mb-3 w-full max-w-4xl min-w-0 mx-auto" data-testid="chat-reminder-card">
      <div className="ct-card" style={{ "--ct-card-tone": "var(--ct-tone-delayed)" } as React.CSSProperties}>
        <div className="ct-card__head">
          <span className="ct-card__icon"><AlarmClock size={14} strokeWidth={2} aria-hidden="true" /></span>
          <span className="min-w-0 flex-1">
            <span className="ct-card__title">
              Reminder
              {whoSet && <span className="ct-card__meta">from {whoSet}</span>}
              {model && <span className="ct-card__meta">· {model}</span>}
            </span>
            {subline && (
              <span
                className="block truncate text-[11px]"
                style={{ color: "var(--ct-muted-fg)" }}
                title={sublineTitle || undefined}
              >
                {subline}
              </span>
            )}
            {repeatLine && (
              <span className="block truncate text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
                {repeatLine}
              </span>
            )}
          </span>
          {every && id && onStop && (
            <button
              type="button"
              aria-label="Stop recurring reminder"
              title="Stop recurring reminder"
              onClick={() => onStop(id)}
              className="ct-bar-pill shrink-0"
              style={{ "--ct-tab-pill-h": "24px" } as React.CSSProperties}
            >
              <X size={12} aria-hidden="true" />
              <span>Stop</span>
            </button>
          )}
        </div>
        {body.trim() && (
          <div className="ct-card__body" data-reminder-id={id}>
            {renderUserContent(body)}
          </div>
        )}
      </div>
    </div>
  );
}
