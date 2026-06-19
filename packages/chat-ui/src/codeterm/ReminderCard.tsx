import React from "react";
import { AlarmClock } from "lucide-react";
import { renderUserContent } from "../MarkdownBody";
import { absoluteTime, relativeTime } from "./relativeTime";

interface Props {
  whoSet?: string;
  model?: string;
  setAt?: string;
  fireAt?: string;
  id?: string;
  body: string;
}

export function ReminderCard({ whoSet, model, setAt, fireAt, id, body }: Props): React.ReactElement {
  const setRel = relativeTime(setAt);
  const fireRel = relativeTime(fireAt);
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
      <div
        className="max-w-full min-w-0 rounded-md border"
        style={{
          background: "color-mix(in srgb, var(--ct-surface, #161b22) 90%, var(--ct-accent, #58a6ff) 10%)",
          borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 32%, transparent)",
          color: "var(--ct-fg)",
        }}
      >
        <div className="flex min-w-0 items-center gap-2 px-3 py-2">
          <AlarmClock size={14} strokeWidth={2} style={{ color: "var(--ct-accent, #58a6ff)" }} />
          <span className="min-w-0 flex-1">
            <span className="flex items-center gap-1.5 text-xs font-medium">
              Reminder
              {whoSet && (
                <span style={{ color: "var(--ct-muted-fg)" }}>from {whoSet}</span>
              )}
              {model && (
                <span className="text-[10px]" style={{ color: "var(--ct-muted-fg)" }}>
                  · {model}
                </span>
              )}
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
          </span>
        </div>
        {body.trim() && (
          <div
            className="border-t px-3 py-2 text-xs"
            style={{ borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 22%, transparent)" }}
            data-reminder-id={id}
          >
            {renderUserContent(body)}
          </div>
        )}
      </div>
    </div>
  );
}
