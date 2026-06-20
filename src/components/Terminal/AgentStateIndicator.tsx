import type { ReactNode } from "react";
import {
  AlertTriangle,
  ArrowUpCircle,
  Cpu,
  HelpCircle,
  KeyRound,
  Layers,
  Moon,
  ShieldQuestion,
  Timer,
} from "lucide-react";

// The live state of a pane's agent, as a single small shape. `subtype` only
// carries meaning for the `question` state; the others ignore it. The component
// is built to render the full shape now even though the backend signal that
// drives every branch is wired separately.
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
  /** Idle for long enough that the pane reads as dormant rather than just done. */
  longIdle?: boolean;
}

// A handful of working verbs so identical adjacent panes don't read as one
// stuck label. Deterministic per pane — not animated cycling, which reads busy.
const WORKING_VERBS = ["Working", "Thinking", "Reasoning", "Cooking", "Crunching"] as const;

function workingVerb(seed: string | undefined): string {
  if (!seed) return WORKING_VERBS[0];
  let h = 0;
  for (let i = 0; i < seed.length; i++) h = (h * 31 + seed.charCodeAt(i)) | 0;
  return WORKING_VERBS[Math.abs(h) % WORKING_VERBS.length];
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

// Block cursor — the unifying signature across every state. Class drives the
// motion (breathe / blink / steady); colour comes from the caller's context.
function Cursor({ variant, color }: { variant: "work" | "idle" | "rest" | "solid"; color: string }) {
  const cls =
    variant === "work"
      ? "ct-agent-cursor ct-agent-cursor-work"
      : variant === "idle"
        ? "ct-agent-cursor ct-agent-cursor-idle"
        : variant === "rest"
          ? "ct-agent-cursor ct-agent-cursor-rest"
          : "ct-agent-cursor";
  return <span aria-hidden className={cls} style={{ color }} />;
}

// Inline, in-canvas agent state line. Never a bubble — it sits in the
// conversation flow as the pane's live pulse. Accessible: announced politely,
// motion is CSS-gated by prefers-reduced-motion.
export function AgentStateIndicator({
  state,
  paneSeed,
  className,
}: {
  state: AgentState;
  /** Stable per-pane string used only to vary the working verb. */
  paneSeed?: string;
  className?: string;
}) {
  if (state.state === "working") {
    const verb = workingVerb(paneSeed);
    return (
      <div
        className={`flex items-center gap-2 py-1.5 ${className ?? ""}`}
        data-testid="agent-state"
        data-agent-state="working"
        role="status"
        aria-live="polite"
        aria-label={`Agent ${verb.toLowerCase()}`}
      >
        <Cursor variant="work" color="var(--ct-accent, #58a6ff)" />
        <span
          className="ct-agent-shimmer text-sm font-medium tracking-tight"
          style={{ fontFamily: "var(--ct-font-mono)" }}
        >
          {verb}
          <span className="opacity-70">…</span>
        </span>
      </div>
    );
  }

  if (state.state === "idle") {
    const rest = state.longIdle === true;
    return (
      <div
        className={`flex items-center gap-2 py-1.5 ${className ?? ""}`}
        data-testid="agent-state"
        data-agent-state={rest ? "idle-long" : "idle"}
        role="status"
        aria-live="polite"
        aria-label={rest ? "Agent resting" : "Your turn"}
      >
        {rest ? (
          <Moon size={14} strokeWidth={2} aria-hidden style={{ color: "var(--ct-muted-fg)" }} />
        ) : (
          <Cursor variant="idle" color="var(--ct-green, var(--ct-ok, #3fb950))" />
        )}
        <span
          className="text-sm tracking-tight"
          style={{
            fontFamily: "var(--ct-font-mono)",
            color: rest ? "var(--ct-muted-fg)" : "var(--ct-fg)",
            opacity: rest ? 0.75 : 0.9,
          }}
        >
          {rest ? "Resting" : "Your turn"}
        </span>
      </div>
    );
  }

  const copy = questionCopy(state.subtype);
  const color = toneColor(copy.tone);
  return (
    <div
      className={`ct-agent-question-in mt-2 flex items-start gap-2.5 rounded-lg px-3 py-2.5 ${className ?? ""}`}
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
        style={{
          background: `color-mix(in srgb, ${color} 16%, transparent)`,
          color,
        }}
      >
        {copy.icon}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <span className="text-sm font-semibold leading-tight" style={{ color: "var(--ct-fg)" }}>
            {copy.title}
          </span>
          <Cursor variant="solid" color={color} />
        </div>
        <p className="mt-0.5 text-xs leading-snug" style={{ color: "var(--ct-muted-fg)" }}>
          {copy.hint}
        </p>
      </div>
    </div>
  );
}
