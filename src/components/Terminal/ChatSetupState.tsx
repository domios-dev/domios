import { useCallback, useEffect, useRef, useState } from "react";
import { ProviderGlyph } from "../ProviderGlyph";
import { DeferredLoader } from "../shared/DeferredLoader";
import type { BootStage, Tab } from "../../lib/types";
import type { TabBootOverlayView } from "./bootOverlay";

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

export function chatSetupSubject(tab: Tab | null | undefined, providerName: string): string {
  if (tab?.role === "Orchestrator") return "Orchestrator";
  if (tab?.role === "Worker") return "Worker";
  if (tab?.role === "Manager" && tab.role_profile === "planner") return "Planner";
  if (tab?.role === "Manager") return "Manager";
  return providerName;
}

export function chatSetupStageLabel(stage: BootStage | null | undefined, subject: string): string {
  if (!stage) return "Preparing launch…";
  switch (stage) {
    case "pty_starting":
      return "Creating tab…";
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
      return "Connecting to terminal…";
  }
  return unreachableBootStage(stage);
}

function unreachableBootStage(stage: never): never {
  throw new Error(`Unexpected boot stage: ${stage}`);
}

export function chatSetupView(
  tab: Tab | null | undefined,
  chatSessionAttached = false,
): TabBootOverlayView {
  if (chatSessionAttached) return { kind: "none" };
  const stage = tab?.boot_stage;
  if (stage === "spawn_failed") {
    return { kind: "starting", label: "Starting" };
  }
  if (tab?.agent_booting && !stage) return { kind: "starting", label: "Starting" };
  if (stage === "agent_ready" || stage === "pty_reattached" || !stage) return { kind: "none" };
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
  tab: Tab | null | undefined;
  view: TabBootOverlayView;
  providerName: string;
  providerIconSvg?: string | null;
  providerIconColor?: string | null;
  providerIconHint?: string | null;
  onEscape?: () => void;
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

export function ChatSetupState({
  tab,
  view,
  providerName,
  providerIconSvg,
  providerIconColor,
  providerIconHint,
  onEscape,
}: ChatSetupStateProps) {
  const cardRef = useRef<HTMLDivElement>(null);
  const [transitionState, setTransitionState] = useState<"hidden" | "present" | "exiting">(
    view.kind === "none" ? "hidden" : "present",
  );
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

  if (view.kind !== "starting" && transitionState !== "exiting") return null;

  const progress = tab?.boot_stage ? STAGE_PROGRESS[tab.boot_stage] : STAGE_PROGRESS.pty_starting;
  return (
    <DeferredLoader ref={cardRef} state={transitionState} className="chat-launch-overlay flex items-center justify-center p-4" data-testid="chat-setup-state" onKeyDown={handleKeyDown} onTransitionEnd={handleTransitionEnd} style={cardStyle}>
      <div className="flex w-full max-w-sm flex-col items-center text-center">
        <div className="chat-launch-ring" data-testid="chat-launch-ring" style={{ "--launch-progress": `${progress}%` } as React.CSSProperties}>
          <LaunchGlyph providerName={providerName} iconSvg={providerIconSvg} iconColor={providerIconColor} iconHint={providerIconHint} />
          <span className="chat-launch-ticks" data-testid="chat-launch-ticks" aria-hidden>{Array.from({ length: 8 }, (_, index) => <i key={index} />)}</span>
        </div>
      </div>
    </DeferredLoader>
  );
}
