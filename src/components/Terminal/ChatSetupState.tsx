import { tabActivity } from "../../lib/tabActivity";
import { useCallback } from "react";
import { ProviderGlyph } from "../ProviderGlyph";
import { DeferredLoader } from "../shared/DeferredLoader";
import type { BootStage, Tab } from "../../lib/types";
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
  awaiting_owner_prompt: 76,
  pty_reattached: 100,
  launch_deadline_elapsed: 92,
  launch_binding_unconfirmed: 92,
  spawn_failed: 0,
};

export function chatSetupView(
  tab: Tab | null | undefined,
  chatSessionAttached = false,
): ChatBodyView {
  if (chatSessionAttached) return { kind: "none" };
  const activity = tabActivity(tab);
  return activity.launching || activity.kind === "failed"
    ? { kind: "starting", label: activity.label }
    : { kind: "none" };
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
