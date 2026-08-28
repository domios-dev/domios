import { Square, Terminal, X } from "lucide-react";
import { ActivityDots } from "../shared/ActivityDots";

export type AgentStateKind = "working" | "idle" | "quiet" | "question";

export type AgentQuestionSubtype =
  | "model_choice"
  | "model_picker"
  | "permission"
  | "auth"
  | "rate_limit"
  | "update"
  | "error"
  | "unknown";

export interface AgentState {
  state: AgentStateKind;
  subtype?: AgentQuestionSubtype;
  status_line?: string | null;
  phase?: string | null;
}

type WorkPhase = "thinking" | "streaming" | "work";
const STATUS_MESSAGE_MAX_CHARS = 80;

function workPhaseOf(phase: string | null | undefined): WorkPhase {
  return phase === "thinking" || phase === "streaming" ? phase : "work";
}

function compactStatusMessage(text: string): string {
  const chars = Array.from(text);
  if (chars.length <= STATUS_MESSAGE_MAX_CHARS) return text;
  return `${chars.slice(0, STATUS_MESSAGE_MAX_CHARS - 1).join("").trimEnd()}…`;
}

function questionStatus(subtype: AgentQuestionSubtype | undefined): string {
  switch (subtype) {
    case "model_choice":
    case "model_picker":
      return "Choose a model";
    case "permission":
      return "Permission needed";
    case "auth":
      return "Sign in to continue";
    case "rate_limit":
      return "Rate limited";
    case "update":
      return "Update available";
    case "error":
      return "Needs attention";
    default:
      return "Waiting for input";
  }
}

export function AgentStateIndicator({
  state,
  providerName,
  model,
  className,
  onStop,
  onCancel,
  onOpenTerminal,
}: {
  state: AgentState;
  providerName?: string | null;
  model?: string | null;
  className?: string;
  onStop?: () => void;
  onCancel?: () => void;
  onOpenTerminal?: () => void;
}) {
  const isWorking = state.state === "working";
  const isQuiet = state.state === "quiet";
  const isQuestion = state.state === "question";
  const phase = workPhaseOf(state.phase);
  const status = state.status_line?.trim()
    || (isWorking ? "Working" : isQuiet ? "Session quiet — rebind to continue" : isQuestion ? questionStatus(state.subtype) : "");
  const text = [providerName?.trim(), model?.trim(), status].filter(Boolean).join(" · ");

  return (
    <div
      className={`ct-agent-row ct-agent-row--chat flex items-center gap-2 ${className ?? ""}`}
      data-testid="agent-state"
      data-agent-state={state.state}
      data-agent-subtype={state.subtype ?? undefined}
      data-agent-phase={phase}
      role="status"
      aria-live="polite"
      aria-label={text ? `Agent ${text}` : "Agent idle"}
    >
      <ActivityDots
        size="small"
        tone={isWorking ? (phase === "thinking" ? "thinking" : "accent") : "muted"}
        rhythm={phase}
        idle={!isWorking}
      />
      <span
        className="ct-agent-msg min-w-0 flex-1 truncate text-sm font-medium tracking-tight"
        style={{ fontFamily: "var(--ct-font-mono)" }}
        title={text || undefined}
      >
        {compactStatusMessage(text)}
      </span>
      {isWorking && onStop && (
        <button
          type="button"
          data-testid="agent-stop"
          onClick={onStop}
          className="ct-agent-stop shrink-0 inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-xs font-medium"
          aria-label="Stop the agent"
          title="Stop the agent"
        >
          <Square size={9} strokeWidth={0} fill="currentColor" aria-hidden />
          Stop
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
