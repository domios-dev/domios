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

const TONE_CHIP: Record<Tone, { label: string; color: string }> = {
  ok: { label: "Ok", color: "var(--ct-green)" },
  warn: { label: "Attention", color: "var(--ct-yellow)" },
  danger: { label: "Stalled", color: "var(--ct-red)" },
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
        className="ct-card"
        style={{ "--ct-card-tone": chip.color } as React.CSSProperties}
      >
        <div className="ct-card__head">
          <span className="ct-card__icon"><Scale size={14} strokeWidth={2} aria-hidden="true" /></span>
          <span className="ct-card__title">
            Watcher verdict
            <span data-testid="watcher-verdict-status-chip" className="ct-card__chip" style={{ color: chip.color }}>
              {chip.label}
            </span>
          </span>
        </div>
        <div className="ct-card__body">
          <p className="text-sm leading-relaxed">{verdict.summary}</p>
          {verdict.actions.length > 0 && (
            <ul data-testid="watcher-verdict-actions" className="mt-2 space-y-1.5 text-[12px]">
              {verdict.actions.map((action, index) => (
                <li key={`${action.kind}-${action.tab ?? ""}-${index}`}>
                  <span className="font-medium" style={{ color: chip.color }}>{action.kind}</span>
                  {action.tab ? <span style={{ color: "var(--ct-muted-fg)" }}>{` → ${action.tab}`}</span> : null}
                  <span className="block whitespace-pre-wrap break-words">{action.message}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}

export const WatcherVerdictCard = React.memo(WatcherVerdictCardImpl);
