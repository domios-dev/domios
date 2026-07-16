import { useCallback, useEffect, useRef, useState } from "react";
import { ProviderGlyph } from "../ProviderGlyph";
import type { Pane } from "../../lib/types";
import { paneBootOverlayView, type PaneBootOverlayView } from "./bootOverlay";

const STAGE_PROGRESS: Record<string, number> = {
  spawn_accepted: 16,
  pty_starting: 16,
  pty_spawned: 32,
  session_attached: 50,
  shell_ready: 50,
  preparing_role_instructions: 72,
  agent_launching: 72,
  delivering_instructions: 88,
  ready: 100,
  agent_ready: 100,
};

export function chatSetupSubject(pane: Pane | null | undefined, providerName: string): string {
  if (pane?.role === "Orchestrator") return "Orchestrator";
  if (pane?.role === "Worker") return "Worker";
  if (pane?.role === "Manager" && pane.role_profile === "planner") return "Planner";
  if (pane?.role === "Manager") return "Manager";
  return providerName;
}

export function chatSetupStageLabel(stage: Pane["boot_stage"], subject: string): string {
  switch (stage as string | null | undefined) {
    case "spawn_accepted":
    case "pty_starting":
    case "pty_spawned":
      return "Spawn accepted…";
    case "session_attached":
    case "shell_ready":
      return "Session attached…";
    case "preparing_role_instructions":
    case "agent_launching":
      return `Preparing ${subject.toLowerCase()} instructions…`;
    case "delivering_instructions":
      return "Delivering instructions…";
    default:
      return "Preparing launch…";
  }
}

export function chatSetupView(pane: Pane | null | undefined): PaneBootOverlayView {
  return paneBootOverlayView(pane);
}

interface ChatSetupStateProps {
  pane: Pane | null | undefined;
  view: PaneBootOverlayView;
  providerName: string;
  providerIconSvg?: string | null;
  providerIconColor?: string | null;
  providerIconHint?: string | null;
  onRetry?: () => void;
  onOpenTerminal?: () => void;
  onEscape?: () => void;
}

function projectFact(pane: Pane | null | undefined): string {
  const cwd = pane?.cwd;
  if (!cwd) return "workspace";
  const normalized = cwd.replace(/\\/g, "/").replace(/\/$/, "");
  return normalized.slice(normalized.lastIndexOf("/") + 1) || "workspace";
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
  onRetry,
  onOpenTerminal,
  onEscape,
}: ChatSetupStateProps) {
  const retryRef = useRef<HTMLButtonElement>(null);
  const [failureCount, setFailureCount] = useState(0);
  const wasFailed = useRef(false);
  const subject = chatSetupSubject(pane, providerName);
  const isFailed = view.kind === "failed";

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

  if (isFailed) {
    return (
      <div className="flex justify-center px-4 py-10" data-testid="chat-launch-failed" onKeyDown={handleKeyDown}>
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
            {onRetry && <button ref={retryRef} type="button" data-testid="chat-launch-retry" onClick={onRetry} className="min-h-10 rounded-md border px-3 text-xs font-medium" style={{ borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}>Retry</button>}
            {failureCount >= 2 && onOpenTerminal && <button type="button" data-testid="chat-launch-show-log" onClick={onOpenTerminal} className="min-h-10 rounded-md border px-3 text-xs font-medium" style={{ borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}>Show log</button>}
          </div>
        </div>
      </div>
    );
  }

  if (view.kind !== "starting") return null;

  const progress = STAGE_PROGRESS[pane?.boot_stage ?? ""] ?? 16;
  const facts = [providerName, pane?.agent_session_model, projectFact(pane)].filter(Boolean).join(" · ");
  return (
    <div className="flex justify-center px-4 py-12" data-testid="chat-setup-state" onKeyDown={handleKeyDown}>
      <div className="flex w-full max-w-sm flex-col items-center text-center" style={{ transition: "opacity 120ms ease" }}>
        <div className="chat-launch-ring" data-testid="chat-launch-ring" style={{ "--launch-progress": `${progress}%` } as React.CSSProperties}>
          <LaunchGlyph providerName={providerName} iconSvg={providerIconSvg} iconColor={providerIconColor} iconHint={providerIconHint} />
        </div>
        <div className="mt-4 text-[10px] font-semibold uppercase tracking-[0.16em]" data-testid="chat-launch-eyebrow" style={{ color: "var(--ct-muted-fg)" }}>Team</div>
        <div className="mt-1 text-sm font-semibold" data-testid="chat-launch-role" style={{ color: "var(--ct-fg)" }}>{subject}</div>
        <div className="mt-1 font-mono text-[11px]" data-testid="chat-launch-facts" style={{ color: "var(--ct-muted-fg)" }}>{facts}</div>
        <div className="mt-4 min-h-5 text-xs" data-testid="chat-setup-stage" aria-live="polite" style={{ color: "var(--ct-fg)" }}>
          {chatSetupStageLabel(pane?.boot_stage, subject)}
        </div>
      </div>
    </div>
  );
}
