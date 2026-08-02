import { type ReactNode, useEffect, useRef, useState } from "react";
import { AGENT_WORKING_LINE_SEP, formatAgentWorkingLine } from "@codeterm/chat-ui";
import {
  AlertTriangle,
  ArrowUpCircle,
  Cpu,
  HelpCircle,
  KeyRound,
  Layers,
  ShieldQuestion,
  Square,
  Terminal,
  Timer,
  X,
} from "lucide-react";
import { ActivityDots } from "../shared/ActivityDots";

export type AgentStateKind = "working" | "idle" | "question";

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
  last_turn_summary?: string | null;
  phase?: string | null;
  elapsed_ms?: number | null;
  /** Idle for long enough that the tab reads as dormant rather than just done. */
  longIdle?: boolean;
}

export function shouldRenderAgentStateIndicator(
  state: AgentStateKind,
  hasMessages: boolean,
  authoritativeQuestion: boolean,
): boolean {
  return state === "question" ? authoritativeQuestion : hasMessages;
}

type Tone = "accent" | "warn" | "info" | "danger" | "ok";

function toneColor(tone: Tone): string {
  switch (tone) {
    case "warn":
      return "var(--ct-warn, var(--ct-amber, #d29922))";
    case "info":
      return "var(--ct-blue, var(--ct-accent, #58a6ff))";
    case "danger":
      return "var(--ct-err, var(--ct-red, #f85149))";
    case "ok":
      return "var(--ct-green, var(--ct-ok, #3fb950))";
    default:
      return "var(--ct-accent, #58a6ff)";
  }
}

interface QuestionCopy {
  icon: ReactNode;
  title: string;
  hint: string;
  tone: Tone;
}

// Each subtype is a deliberate icon + microcopy pair, in the interface's voice.
// Copy names what the person controls and what to do next — never the wire shape.
function questionCopy(subtype: AgentQuestionSubtype | undefined): QuestionCopy {
  const sz = 15;
  switch (subtype) {
    case "model_choice":
      return {
        icon: <Cpu size={sz} strokeWidth={2} />,
        title: "Choose a model",
        hint: "Pick which model should handle this.",
        tone: "accent",
      };
    case "model_picker":
      return {
        icon: <Layers size={sz} strokeWidth={2} />,
        title: "Select a model",
        hint: "Open the model list and choose one.",
        tone: "accent",
      };
    case "permission":
      return {
        icon: <ShieldQuestion size={sz} strokeWidth={2} />,
        title: "Permission needed",
        hint: "Approve this action to let work continue.",
        tone: "warn",
      };
    case "auth":
      return {
        icon: <KeyRound size={sz} strokeWidth={2} />,
        title: "Sign in to continue",
        hint: "Authenticate to resume the session.",
        tone: "info",
      };
    case "rate_limit":
      return {
        icon: <Timer size={sz} strokeWidth={2} />,
        title: "Rate limited",
        hint: "Waiting for the limit to reset.",
        tone: "warn",
      };
    case "update":
      return {
        icon: <ArrowUpCircle size={sz} strokeWidth={2} />,
        title: "Update available",
        hint: "A newer version is ready to install.",
        tone: "info",
      };
    case "error":
      return {
        icon: <AlertTriangle size={sz} strokeWidth={2} />,
        title: "Needs attention",
        hint: "Something went wrong — check the details below.",
        tone: "danger",
      };
    default:
      return {
        icon: <HelpCircle size={sz} strokeWidth={2} />,
        title: "Waiting for you",
        hint: "The agent needs your input to keep going.",
        tone: "accent",
      };
  }
}

// Steady caret beside the question title — the one place the block cursor remains.
function Cursor({ color }: { color: string }) {
  return <span aria-hidden className="ct-agent-cursor" style={{ color }} />;
}

// changes the state.
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

function useStablePhase(phase: WorkPhase, dwellMs = 700): WorkPhase {
  const [shown, setShown] = useState(phase);
  const shownRef = useRef(shown);
  shownRef.current = shown;
  useEffect(() => {
    if (phase === shownRef.current) return;
    const t = setTimeout(() => setShown(phase), dwellMs);
    return () => clearTimeout(t);
  }, [phase, dwellMs]);
  return shown;
}

export function AgentStateIndicator({
  state,
  providerName,
  model,
  className,
  questionGeneration,
  onStop,
  onCancel,
  onOpenTerminal,
}: {
  state: AgentState;
  providerName?: string | null;
  model?: string | null;
  className?: string;
  questionGeneration?: number | null;
  /** Interrupt the working agent (PTY tabs only). Renders a Stop control while
   *  working when provided. */
  onStop?: () => void;
  /** Cancel a terminal-side question/picker (PTY tabs only). */
  onCancel?: () => void;
  /** Reveal the live terminal — the only place to answer a structural question
   *  that has no inline card. Renders an 'Open terminal' control while
   *  questioning when provided. */
  onOpenTerminal?: () => void;
}) {
  const isWorking = state.state === "working";
  const summary = state.last_turn_summary?.trim();
  const [dismissedQuestionGeneration, setDismissedQuestionGeneration] = useState<number | null>(null);
  // Hook runs every render (rules of hooks); only meaningful while working.
  const phase = useStablePhase(isWorking ? workPhaseOf(state.phase) : "work");

  if (state.state === "question") {
    if (
      dismissedQuestionGeneration === (questionGeneration ?? -1)
    ) {
      return null;
    }
    const copy = questionCopy(state.subtype);
    const color = toneColor(copy.tone);
    return (
      <div
        className={`ct-agent-question-in relative mt-2 flex items-start gap-2.5 rounded-lg px-3 py-2.5 ${className ?? ""}`}
        data-testid="agent-state"
        data-agent-state="question"
        data-agent-subtype={state.subtype ?? "unknown"}
        role="status"
        aria-live="polite"
        style={{
          background: `color-mix(in srgb, ${color} 9%, var(--ct-surface, #161b22))`,
          border: `1px solid color-mix(in srgb, ${color} 32%, transparent)`,
          borderLeft: `3px solid ${color}`,
        }}
      >
        <span
          aria-hidden
          className="mt-0.5 inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md"
          style={{ background: `color-mix(in srgb, ${color} 16%, transparent)`, color }}
        >
          {copy.icon}
        </span>
        <button
          type="button"
          onClick={() => setDismissedQuestionGeneration(questionGeneration ?? -1)}
          aria-label="Dismiss waiting card"
          title="Dismiss waiting card"
          className="ct-agent-dismiss-question absolute right-2 top-2 inline-flex h-6 w-6 items-center justify-center rounded-md"
        >
          <X size={14} strokeWidth={2} aria-hidden />
        </button>
        <div className="min-w-0 flex-1 pr-6">
          <div className="flex items-center gap-1.5">
            <span className="text-sm font-semibold leading-tight" style={{ color: "var(--ct-fg)" }}>
              {copy.title}
            </span>
            <Cursor color={color} />
          </div>
          <p className="mt-0.5 text-xs leading-snug" style={{ color: "var(--ct-muted-fg)" }}>
            {copy.hint}
          </p>
          {(onOpenTerminal || onCancel) && (
            <div className="mt-2 flex flex-wrap items-center gap-1.5">
              {onOpenTerminal && (
                <button
                  type="button"
                  data-testid="agent-open-terminal"
                  onClick={onOpenTerminal}
                  className="ct-agent-open-term inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium"
                  style={{
                    color,
                    border: `1px solid color-mix(in srgb, ${color} 38%, transparent)`,
                    background: `color-mix(in srgb, ${color} 12%, transparent)`,
                  }}
                >
                  <Terminal size={13} strokeWidth={2} aria-hidden />
                  Open terminal
                </button>
              )}
              {onCancel && (
                <button
                  type="button"
                  data-testid="agent-cancel-question"
                  onClick={onCancel}
                  className="ct-agent-cancel inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium"
                  aria-label="Cancel the terminal question"
                  title="Cancel the terminal question"
                >
                  <X size={13} strokeWidth={2} aria-hidden />
                  Cancel
                </button>
              )}
            </div>
          )}
        </div>
      </div>
    );
  }

  const identity = [providerName?.trim(), model?.trim()].filter(Boolean).join(AGENT_WORKING_LINE_SEP);
  const rawActivity = isWorking ? (state.status_line?.trim() || "Working") : (summary ?? "");
  const text =
    formatAgentWorkingLine({
      providerName,
      model,
      activityText: rawActivity,
    }) ?? identity;
  const visibleText = compactStatusMessage(text);
  return (
    <div
      className={`ct-agent-row flex items-baseline gap-2 py-1 ${className ?? ""}`}
      data-testid="agent-state"
      data-agent-state={isWorking ? "working" : "idle"}
      data-agent-phase={phase}
      role="status"
      aria-live="polite"
      aria-label={text ? `Agent ${text}` : "Agent status"}
    >
      <ActivityDots
        size="small"
        tone={phase === "thinking" ? "thinking" : "accent"}
        rhythm={phase}
        idle={!isWorking}
      />
      {/* keyed by state (not text) so it fades in on working↔idle, never on a status tick */}
      <span
        key={isWorking ? "working" : "idle"}
        className="ct-agent-msg min-w-0 flex-1 truncate text-sm font-medium tracking-tight"
        style={{ fontFamily: "var(--ct-font-mono)" }}
        title={text || undefined}
      >
        {visibleText}
        {isWorking && !state.status_line && <span className="opacity-70">…</span>}
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
    </div>
  );
}
