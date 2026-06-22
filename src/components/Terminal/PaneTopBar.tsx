import { PaneModeSwitch } from "./PaneModeSwitch";
import { ChatDefaultSuggestion } from "./ChatDefaultSuggestion";
import { UsageBubble } from "../Usage/UsageBubble";
import { PluginBubbles } from "../Extensions/PluginBubbles";
import { useConfig } from "../../hooks/useConfig";

/** Single height rhythm for every control in the bar (switch + pills). */
export const PANE_BAR_CONTROL_H = 27;

interface PaneTopBarProps {
  paneId: string;
  cwd: string | null;
  /** Show the Terminal | Chat switch (chat-capable PTY panes only). */
  showSwitch: boolean;
  /** Usage provider id, or null to omit the usage pill. */
  usageProvider?: string | null;
  usageVisible?: boolean;
}

/**
 * Pane header strip: mode switch on the left, status pills (usage + plugins) on
 * the right. Every item shares `PANE_BAR_CONTROL_H` — published to CSS-driven
 * children via the `--ct-pane-pill-h` custom property.
 */
export function PaneTopBar({ paneId, cwd, showSwitch, usageProvider, usageVisible }: PaneTopBarProps) {
  const { config } = useConfig();
  const chatFirst = config.ui?.default_new_agent_pane_chat === true;
  return (
    <div
      data-testid="pane-top-bar"
      className="flex shrink-0 items-center justify-between gap-2 px-2"
      style={{
        ["--ct-pane-pill-h" as string]: `${PANE_BAR_CONTROL_H}px`,
        paddingTop: "calc(6px + var(--ct-safe-top, 0px))",
        paddingBottom: 6,
        background: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 3%, transparent)",
        borderBottom: "1px solid var(--ct-border, #30363d)",
      }}
    >
      <div className="flex min-w-0 items-center gap-2">
        {showSwitch && <PaneModeSwitch paneId={paneId} height={PANE_BAR_CONTROL_H} chatFirst={chatFirst} />}
        {showSwitch && <ChatDefaultSuggestion paneId={paneId} height={PANE_BAR_CONTROL_H} />}
      </div>
      <div className="flex min-w-0 items-center gap-1.5">
        <PluginBubbles cwd={cwd} paneId={paneId} />
        {usageProvider && <UsageBubble provider={usageProvider} trackingEnabled={!!usageVisible} />}
      </div>
    </div>
  );
}
