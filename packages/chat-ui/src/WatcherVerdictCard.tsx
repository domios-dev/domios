import * as React from "react";
import { parseWatcherVerdictContent } from "@codeterm/chat-core";
import { Scale } from "lucide-react";

type Tone = "ok" | "warn" | "danger";

function verdictTone(status: "ok" | "attention" | "stalled"): Tone {
  switch (status) {
    case "ok":
      return "ok";
    case "attention":
      return "warn";
    case "stalled":
      return "danger";
  }
}

const TONE_CHIP: Record<Tone, { label: string; color: string; bg: string; border: string }> = {
  ok: {
    label: "OK",
    color: "var(--ct-green, #3fb950)",
    bg: "color-mix(in srgb, var(--ct-surface, #161b22) 88%, var(--ct-green, #3fb950) 12%)",
    border: "color-mix(in srgb, var(--ct-green, #3fb950) 35%, transparent)",
  },
  warn: {
    label: "ATTENTION",
    color: "var(--ct-yellow, #d29922)",
    bg: "color-mix(in srgb, var(--ct-surface, #161b22) 88%, var(--ct-yellow, #d29922) 12%)",
    border: "color-mix(in srgb, var(--ct-yellow, #d29922) 35%, transparent)",
  },
  danger: {
    label: "STALLED",
    color: "var(--ct-red, #f85149)",
    bg: "color-mix(in srgb, var(--ct-surface, #161b22) 88%, var(--ct-red, #f85149) 12%)",
    border: "color-mix(in srgb, var(--ct-red, #f85149) 35%, transparent)",
  },
};

interface WatcherVerdictCardProps {
  content: string;
}

function WatcherVerdictCardImpl({ content }: WatcherVerdictCardProps) {
  const verdict = parseWatcherVerdictContent(content);
  const tone = verdictTone(verdict.status);
  const chip = TONE_CHIP[tone];

  return (
    <div className="mb-3 w-full max-w-4xl min-w-0 mx-auto">
      <div
        data-testid="chat-watcher-verdict-card"
        data-verdict-tone={tone}
        className="max-w-full min-w-0 rounded-md border"
        style={{
          borderColor: chip.border,
          background: chip.bg,
          color: "var(--ct-fg)",
        }}
      >
        <div className="flex min-w-0 items-start gap-2 px-3 py-2">
          <Scale size={14} strokeWidth={2} className="mt-0.5 shrink-0" style={{ color: chip.color }} />
          <div className="min-w-0 flex-1">
            <div className="mb-1 flex flex-wrap items-center gap-2">
              <span className="text-xs font-medium">Watcher verdict</span>
              <span
                data-testid="watcher-verdict-status-chip"
                className="inline-flex rounded-full border px-2 py-0.5 text-[10px] font-mono font-semibold uppercase tracking-wide"
                style={{ color: chip.color, borderColor: chip.border, background: "transparent" }}
              >
                {chip.label}
              </span>
            </div>
            <p className="text-sm leading-relaxed" style={{ color: "var(--ct-fg)" }}>
              {verdict.summary}
            </p>
            {verdict.actions.length > 0 && (
              <ul
                data-testid="watcher-verdict-actions"
                className="mt-2 space-y-1 border-t pt-2 text-[11px]"
                style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 20%, transparent)" }}
              >
                {verdict.actions.map((action, index) => (
                  <li key={`${action.kind}-${action.tab ?? ""}-${index}`} className="font-mono">
                    <span style={{ color: chip.color }}>{action.kind}</span>
                    {action.tab ? (
                      <span style={{ color: "var(--ct-muted-fg)" }}>{` → ${action.tab}`}</span>
                    ) : null}
                    <span className="block whitespace-pre-wrap break-words" style={{ color: "var(--ct-fg)" }}>
                      {action.message}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

export const WatcherVerdictCard = React.memo(WatcherVerdictCardImpl);
