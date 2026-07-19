import { useCallback, useEffect, useRef, useState } from "react";
import { ProviderGlyph } from "../ProviderGlyph";
import type { BootStage, Pane } from "../../lib/types";
import type { PaneBootOverlayView } from "./bootOverlay";

const STAGE_PROGRESS: Record<BootStage, number> = {
  pty_starting: 8,
  pty_spawned: 20,
  reader_ready: 32,
  shell_ready: 36,
  preparing_role_instructions: 52,
  agent_launching: 68,
  launch_written: 76,
  provider_ready: 92,
  delivering_instructions: 84,
  agent_ready: 100,
  pty_reattached: 100,
  spawn_failed: 0,
};

export function chatSetupSubject(pane: Pane | null | undefined, providerName: string): string {
  if (pane?.role === "Orchestrator") return "Orchestrator";
  if (pane?.role === "Worker") return "Worker";
  if (pane?.role === "Manager" && pane.role_profile === "planner") return "Planner";
  if (pane?.role === "Manager") return "Manager";
  return providerName;
}

export function chatSetupStageLabel(stage: BootStage | null | undefined, subject: string): string {
  if (!stage) return "Preparing launch…";
  switch (stage) {
    case "pty_starting":
      return "Creating pane…";
    case "pty_spawned":
      return "Spawn accepted…";
    case "reader_ready":
      return "Reader attached…";
    case "shell_ready":
      return "Session attached…";
    case "preparing_role_instructions":
      return `Preparing ${subject.toLowerCase()} instructions…`;
    case "agent_launching":
      return "Launching provider…";
    case "launch_written":
      return "Waiting for provider…";
    case "provider_ready":
      return "Provider ready…";
    case "delivering_instructions":
      return "Delivering instructions…";
    case "agent_ready":
      return "Ready";
    case "pty_reattached":
      return "Session attached…";
    case "spawn_failed":
      return "Launch failed";
  }
  return unreachableBootStage(stage);
}

function unreachableBootStage(stage: never): never {
  throw new Error(`Unexpected boot stage: ${stage}`);
}

export function chatSetupView(
  pane: Pane | null | undefined,
  chatSessionAttached = false,
): PaneBootOverlayView {
  if (chatSessionAttached) return { kind: "none" };
  const stage = pane?.boot_stage;
  if (!stage || stage === "agent_ready" || stage === "pty_reattached") return { kind: "none" };
  if (stage === "spawn_failed") {
    return {
      kind: "failed",
      reason: pane?.boot_failure_reason ?? null,
      canRetry: pane?.protected === true,
    };
  }
  switch (stage) {
    case "pty_starting":
    case "pty_spawned":
    case "reader_ready":
    case "shell_ready":
    case "preparing_role_instructions":
    case "agent_launching":
    case "launch_written":
    case "provider_ready":
    case "delivering_instructions":
      return { kind: "starting", label: "Starting" };
  }
  return unreachableBootStage(stage);
}

interface ChatSetupStateProps {
  pane: Pane | null | undefined;
  view: PaneBootOverlayView;
  providerName: string;
  providerIconSvg?: string | null;
  providerIconColor?: string | null;
  providerIconHint?: string | null;
  teamName?: string | null;
  projectName?: string | null;
  onRetry?: () => void;
  onOpenTerminal?: () => void;
  onEscape?: () => void;
}

function projectFact(pane: Pane | null | undefined, projectName?: string | null): string {
  const context = projectName || pane?.cwd;
  if (!context) return "workspace";
  const normalized = context.replace(/\\/g, "/").replace(/\/$/, "");
  return normalized.slice(normalized.lastIndexOf("/") + 1) || "workspace";
}

function useReducedMotion(): boolean {
  const read = () => typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
  const [reduced, setReduced] = useState(read);
  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => setReduced(query.matches);
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);
  return reduced;
}

interface LaunchGlyphProps {
  providerName: string;
  iconSvg?: string | null;
  iconColor?: string | null;
  iconHint?: string | null;
}

function LaunchGlyph({ providerName, iconSvg, iconColor, iconHint }: LaunchGlyphProps) {
  return (
    <span
      className="chat-launch-glyph"
      data-testid="chat-launch-glyph"
      role="img"
      aria-label={providerName}
      style={{ color: iconColor ?? "var(--ct-accent, #58a6ff)" }}
    >
      <ProviderGlyph iconSvg={iconSvg} iconColor={iconColor} iconHint={iconHint} title={providerName} className="text-[56px]" />
    </span>
  );
}

export function ChatSetupStatusLine({ pane, providerName }: { pane: Pane | null | undefined; providerName: string }) {
  if (chatSetupView(pane).kind !== "starting") return null;
  return (
    <div
      className="flex shrink-0 items-center border-t px-3 py-1.5 text-[11px]"
      data-testid="chat-setup-status-line"
      style={{ borderColor: "var(--ct-border)", color: "var(--ct-muted-fg)" }}
      aria-live="polite"
    >
      {chatSetupStageLabel(pane?.boot_stage, chatSetupSubject(pane, providerName))}
    </div>
  );
}

export function ChatSetupState({
  pane,
  view,
  providerName,
  providerIconSvg,
  providerIconColor,
  providerIconHint,
  teamName,
  projectName,
  onRetry,
  onOpenTerminal,
  onEscape,
}: ChatSetupStateProps) {
  const retryRef = useRef<HTMLButtonElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  const [failureCount, setFailureCount] = useState(0);
  const [transitionState, setTransitionState] = useState<"hidden" | "present" | "exiting">(
    view.kind === "none" ? "hidden" : "present",
  );
  const wasFailed = useRef(false);
  const subject = chatSetupSubject(pane, providerName);
  const isFailed = view.kind === "failed";
  const reducedMotion = useReducedMotion();

  useEffect(() => {
    if (view.kind !== "none") {
      setTransitionState("present");
      return;
    }
    if (reducedMotion) {
      setTransitionState("hidden");
    } else {
      setTransitionState((state) => state === "present" ? "exiting" : state);
    }
  }, [reducedMotion, view.kind]);

  useEffect(() => {
    if (isFailed && !wasFailed.current) setFailureCount((count) => count + 1);
    wasFailed.current = isFailed;
  }, [isFailed]);

  useEffect(() => {
    if (isFailed) retryRef.current?.focus();
  }, [isFailed]);

  const handleKeyDown = useCallback((event: React.KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    onEscape?.();
  }, [onEscape]);

  const handleTransitionEnd = useCallback((event: React.TransitionEvent<HTMLDivElement>) => {
    if (event.target !== cardRef.current || event.propertyName !== "opacity" || transitionState !== "exiting") return;
    setTransitionState("hidden");
  }, [transitionState]);

  if (transitionState === "hidden") return null;

  const cardStyle: React.CSSProperties = {
    opacity: transitionState === "exiting" ? 0 : 1,
    pointerEvents: transitionState === "exiting" ? "none" : undefined,
    transition: reducedMotion ? "none" : "opacity 120ms ease",
  };

  if (isFailed) {
    return (
      <div ref={cardRef} className="chat-launch-overlay flex justify-center px-4 py-10" data-testid="chat-launch-failed" data-transition-state={transitionState} role="alert" aria-label="Launch failed" onKeyDown={handleKeyDown} onTransitionEnd={handleTransitionEnd} style={cardStyle}>
        <div className="flex w-full max-w-sm flex-col items-center text-center">
          <div
            className="chat-launch-ring chat-launch-ring-failed"
            data-testid="chat-launch-ring"
            style={{ "--launch-progress": "72%" } as React.CSSProperties}
          >
            <LaunchGlyph providerName={providerName} iconSvg={providerIconSvg} iconColor={providerIconColor} iconHint={providerIconHint} />
          </div>
          <div className="mt-4 text-sm font-semibold" style={{ color: "var(--ct-fg)" }}>{subject} could not start</div>
          <div className="mt-1 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
            Check the terminal details, then try again.
          </div>
          <div className="mt-4 flex gap-2">
            {view.canRetry && onRetry && <button ref={retryRef} type="button" data-testid="chat-launch-retry" onClick={onRetry} className="min-h-10 rounded-md border px-3 text-xs font-medium" style={{ borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}>Retry</button>}
            {failureCount >= 2 && onOpenTerminal && <button type="button" data-testid="chat-launch-show-log" onClick={onOpenTerminal} className="min-h-10 rounded-md border px-3 text-xs font-medium" style={{ borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}>Show log</button>}
          </div>
        </div>
      </div>
    );
  }

  if (view.kind !== "starting" && transitionState !== "exiting") return null;

  const progress = pane?.boot_stage ? STAGE_PROGRESS[pane.boot_stage] : STAGE_PROGRESS.pty_starting;
  const project = projectFact(pane, projectName);
  const facts = [providerName, pane?.agent_session_model, project].filter(Boolean).join(" · ");
  const eyebrow = teamName?.trim() || `Project: ${project}`;
  return (
    <div ref={cardRef} className="chat-launch-overlay flex justify-center px-4 py-12" data-testid="chat-setup-state" data-transition-state={transitionState} onKeyDown={handleKeyDown} onTransitionEnd={handleTransitionEnd} style={cardStyle}>
      <div className="flex w-full max-w-sm flex-col items-center text-center">
        <div className="chat-launch-ring" data-testid="chat-launch-ring" style={{ "--launch-progress": `${progress}%` } as React.CSSProperties}>
          <LaunchGlyph providerName={providerName} iconSvg={providerIconSvg} iconColor={providerIconColor} iconHint={providerIconHint} />
          <span className="chat-launch-ticks" data-testid="chat-launch-ticks" aria-hidden>{Array.from({ length: 8 }, (_, index) => <i key={index} />)}</span>
        </div>
        <div className="mt-4 text-[10px] font-semibold uppercase" data-testid="chat-launch-eyebrow" style={{ color: "var(--ct-muted-fg)" }}>{eyebrow}</div>
        <div className="mt-1 text-sm font-semibold" data-testid="chat-launch-role" style={{ color: "var(--ct-fg)" }}>{subject}</div>
        <div className="mt-1 font-mono text-[11px]" data-testid="chat-launch-facts" style={{ color: "var(--ct-muted-fg)" }}>{facts}</div>
        <div className="mt-4 min-h-5 text-xs" data-testid="chat-setup-stage" aria-live="polite" style={{ color: "var(--ct-fg)" }}>
          {chatSetupStageLabel(pane?.boot_stage, subject)}
        </div>
      </div>
    </div>
  );
}
