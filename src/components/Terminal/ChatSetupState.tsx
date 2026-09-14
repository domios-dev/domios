import { useCallback } from "react";
import { ProviderGlyph } from "../ProviderGlyph";
import { DeferredLoader } from "../shared/DeferredLoader";
import type { BootStage, Tab } from "../../lib/types";
import { roleKindLabel } from "../../lib/roleKind";
import type { ChatBodyView } from "./chatBodyVisibility";
import type { ChatUnavailableReason } from "./chatSessionAuthority";

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
  launch_deadline_elapsed: 92,
  launch_binding_unconfirmed: 92,
  spawn_failed: 0,
};

export function chatSetupSubject(tab: Tab | null | undefined, providerName: string): string {
  return roleKindLabel(tab) ?? providerName;
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
    case "launch_deadline_elapsed":
    case "launch_binding_unconfirmed":
      return "Waiting for provider…";
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
): ChatBodyView {
  if (chatSessionAttached) return { kind: "none" };
  const stage = tab?.boot_stage;
  if (
    stage === "spawn_failed" ||
    stage === "launch_deadline_elapsed" ||
    stage === "launch_binding_unconfirmed"
  ) {
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

const UNAVAILABLE_COPY: Record<ChatUnavailableReason, string> = {
  no_transcript_source: "This agent has no readable chat transcript for the current session.",
  authority_mismatch: "The chat transcript does not belong to this tab's current session.",
};

interface ChatSetupStateProps {
  tab: Tab | null | undefined;
  view: ChatBodyView;
  providerName: string;
  providerIconSvg?: string | null;
  providerIconColor?: string | null;
  providerIconHint?: string | null;
  onEscape?: () => void;
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
      <ProviderGlyph
        iconSvg={iconSvg}
        iconColor={iconColor}
        iconHint={iconSvg ? iconHint : "◆"}
        title={providerName}
        className={iconSvg ? "text-[56px]" : "text-[18px]"}
      />
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
  const handleKeyDown = useCallback((event: React.KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    onEscape?.();
  }, [onEscape]);

  if (view.kind === "failed") {
    return (
      <div
        className="mx-auto w-full max-w-sm p-4 text-center"
        data-testid="chat-transcript-unavailable"
        role="status"
        aria-label="Chat transcript unavailable"
      >
        <div className="text-sm font-semibold" style={{ color: "var(--ct-fg, #c9d1d9)" }}>
          Chat transcript unavailable
        </div>
        <div className="mt-1 text-xs" style={{ color: "var(--ct-muted-fg, #8b949e)" }}>
          {UNAVAILABLE_COPY[view.reason]}
        </div>
      </div>
    );
  }
  if (view.kind !== "starting") return null;

  const progress = tab?.boot_stage ? STAGE_PROGRESS[tab.boot_stage] : STAGE_PROGRESS.pty_starting;
  return (
    <DeferredLoader state="present" className="chat-launch-overlay flex items-center justify-center p-4" data-testid="chat-setup-state" onKeyDown={handleKeyDown}>
      <div className="flex w-full max-w-sm flex-col items-center text-center">
        <div className="chat-launch-ring" data-testid="chat-launch-ring" style={{ "--launch-progress": `${progress}%` } as React.CSSProperties}>
          <LaunchGlyph providerName={providerName} iconSvg={providerIconSvg} iconColor={providerIconColor} iconHint={providerIconHint} />
          <span className="chat-launch-ticks" data-testid="chat-launch-ticks" aria-hidden>{Array.from({ length: 8 }, (_, index) => <i key={index} />)}</span>
        </div>
      </div>
    </DeferredLoader>
  );
}
