import { AgentIcon } from "../AgentIcon";
import { BusyState } from "../shared/BusyState";
import type { Pane } from "../../lib/types";
import { paneBootOverlayView, type PaneBootOverlayView } from "./bootOverlay";

export function chatSetupStageLabel(stage: Pane["boot_stage"], providerName: string): string {
  switch (stage) {
    case "pty_starting":
    case "pty_spawned":
      return "Preparing the terminal";
    case "shell_ready":
    case "agent_launching":
      return `Launching ${providerName}`;
    default:
      return `Setting up ${providerName}`;
  }
}

export function chatSetupView(pane: Pane | null | undefined): PaneBootOverlayView {
  return paneBootOverlayView(pane);
}

interface ChatSetupStateProps {
  pane: Pane | null | undefined;
  view: PaneBootOverlayView;
  providerId: string | null | undefined;
  providerName: string;
  onOpenTerminal?: () => void;
}

export function ChatSetupState({ pane, view, providerId, providerName, onOpenTerminal }: ChatSetupStateProps) {
  if (view.kind === "failed") {
    return (
      <div className="flex justify-center py-10 px-4" data-testid="chat-launch-failed">
        <div
          className="max-w-sm rounded-lg border px-4 py-3 text-center"
          style={{
            borderColor: "color-mix(in srgb, var(--ct-red, #f85149) 45%, transparent)",
            background: "color-mix(in srgb, var(--ct-bg, #0d1117) 94%, var(--ct-red, #f85149) 6%)",
          }}
        >
          <div className="text-sm font-semibold" style={{ color: "var(--ct-fg)" }}>
            {providerName} failed to launch
          </div>
          {view.reason && (
            <div className="mt-1 text-xs opacity-75 break-words" title={view.reason}>
              {view.reason}
            </div>
          )}
          {onOpenTerminal && (
            <button
              type="button"
              data-testid="chat-launch-failed-open-terminal"
              onClick={onOpenTerminal}
              className="mt-2.5 rounded-md border px-2.5 py-1 text-xs font-medium hover:brightness-125"
              style={{ borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
            >
              Inspect in Terminal
            </button>
          )}
        </div>
      </div>
    );
  }

  if (view.kind !== "starting") return null;

  const stageLabel = chatSetupStageLabel(pane?.boot_stage, providerName);
  return (
    <div
      className="flex flex-col items-center justify-center gap-3 py-12 px-4 text-center"
      data-testid="chat-setup-state"
    >
      <span
        className="flex h-12 w-12 items-center justify-center rounded-xl border chat-setup-glyph"
        style={{
          background: "color-mix(in srgb, var(--ct-surface, #161b22) 86%, var(--ct-accent, #58a6ff) 14%)",
          borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 28%, transparent)",
        }}
        aria-hidden
      >
        <AgentIcon providerId={providerId ?? undefined} className="text-[22px]" />
      </span>
      <BusyState
        labelKey="paneBoot"
        label={stageLabel}
        phase="work"
        inline
        data-testid="chat-setup-stage"
        style={{ color: "var(--ct-fg)", fontSize: 13 }}
      />
      <span className="text-xs" style={{ color: "var(--ct-muted-fg, #8b949e)" }}>
        The conversation opens as soon as the agent is ready.
      </span>
    </div>
  );
}
