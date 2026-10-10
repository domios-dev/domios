import type { ReactNode } from "react";
import { Square, Terminal, X } from "lucide-react";
import { ActivityDots } from "../shared/ActivityDots";
import type { Tab } from "../../lib/types";

import type { TabActivity } from "../../lib/tabActivity";
export type { AgentState } from "../../lib/tabActivity";

const STATUS_MESSAGE_MAX_CHARS = 80;

function compactStatusMessage(text: string): string {
  const chars = Array.from(text);
  if (chars.length <= STATUS_MESSAGE_MAX_CHARS) return text;
  return `${chars.slice(0, STATUS_MESSAGE_MAX_CHARS - 1).join("").trimEnd()}…`;
}

export function AgentStateIndicator({
  activity,
  providerName,
  model,
  reasoningEffort,
  className,
  onStop,
  onCancel,
  onOpenTerminal,
  onRebind,
  renderIdentity,
}: {
  activity: TabActivity;
  providerName?: string | null;
  model?: string | null;
  reasoningEffort?: Tab["reasoning_effort_identity"] | null;
  className?: string;
  onStop?: () => void;
  onCancel?: () => void;
  onOpenTerminal?: () => void;
  onRebind?: () => void;
  renderIdentity?: (label: string) => ReactNode;
}) {
  const isWorking = activity.canStop;
  const isQuiet = activity.quiet;
  const isQuestion = activity.kind === "waiting-on-user";
  const phase = activity.phase;
  const status = activity.label;
  const actualEffort = reasoningEffort?.actual_source === "session-owned"
    ? reasoningEffort.actual?.trim()
    : null;
  const identity = [providerName?.trim(), model?.trim(), actualEffort].filter(Boolean).join(" · ");
  const text = [identity, status].filter(Boolean).join(" · ");
  const dots = (
    <ActivityDots
      size="small"
      tone={isWorking ? (phase === "thinking" ? "thinking" : "accent") : "muted"}
      rhythm={phase}
      active={activity.active}
    />
  );

  return (
    <div
      className={`ct-agent-row ct-agent-row--chat flex items-center gap-2 ${className ?? ""}`}
      data-testid="agent-state"
      data-agent-state={activity.state}
      data-tab-activity={activity.kind}
      data-agent-subtype={activity.subtype ?? undefined}
      data-agent-phase={phase}
      role="status"
      aria-live="polite"
      aria-label={text ? `Agent ${text}` : "Agent idle"}
    >
      {renderIdentity && identity ? (
        <>
          <span
            className="ct-agent-msg ct-agent-identity flex min-w-0 shrink text-sm font-medium tracking-tight"
            style={{ fontFamily: "var(--ct-font-mono)" }}
          >
            {renderIdentity(identity)}
          </span>
          {dots}
          <span
            className="ct-agent-msg min-w-0 flex-1 truncate text-sm font-medium tracking-tight"
            style={{ fontFamily: "var(--ct-font-mono)" }}
            title={status || undefined}
          >
            {status ? compactStatusMessage(status) : ""}
          </span>
        </>
      ) : (
        <>
          {dots}
          <span
            className="ct-agent-msg min-w-0 flex-1 truncate text-sm font-medium tracking-tight"
            style={{ fontFamily: "var(--ct-font-mono)" }}
            title={text || undefined}
          >
            {compactStatusMessage(text)}
          </span>
        </>
      )}
      {onStop && (!isQuiet && !isQuestion) && (
        <button
          type="button"
          data-testid={isWorking ? "agent-stop" : "agent-stop-slot"}
          onClick={isWorking ? onStop : undefined}
          disabled={!isWorking}
          aria-hidden={!isWorking || undefined}
          tabIndex={isWorking ? undefined : -1}
          className={`ct-agent-stop shrink-0 inline-flex items-center gap-1.5 px-2.5 py-0.5 text-xs font-medium${isWorking ? "" : " invisible"}`}
          aria-label="Stop the agent"
          title={isWorking ? "Stop the agent" : undefined}
        >
          <Square size={9} strokeWidth={0} fill="currentColor" aria-hidden />
          <span className="ct-agent-stop__label">Stop</span>
        </button>
      )}
      {onRebind && !isQuestion && (
        <button
          type="button"
          data-testid="agent-rebind"
          onClick={onRebind}
          className="shrink-0 rounded-md px-1.5 py-0.5 text-xs font-medium"
          aria-label="Repair session binding"
          title="Repair session binding"
        >
          Repair
        </button>
      )}
      {isQuestion && onOpenTerminal && (
        <button
          type="button"
          data-testid="agent-open-terminal"
          onClick={onOpenTerminal}
          className="ct-agent-open-term shrink-0 inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-xs font-medium"
        >
          <Terminal size={12} strokeWidth={2} aria-hidden />
          Open terminal
        </button>
      )}
      {isQuestion && onCancel && (
        <button
          type="button"
          data-testid="agent-cancel-question"
          onClick={onCancel}
          className="ct-agent-cancel shrink-0 inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-xs font-medium"
          aria-label="Cancel the terminal question"
          title="Cancel the terminal question"
        >
          <X size={12} strokeWidth={2} aria-hidden />
          Cancel
        </button>
      )}
    </div>
  );
}
