import { useEffect, useRef, useState } from "react";
import { TerminalPane } from "./TerminalPane";
import { isPaneVisible } from "../MemoryPanel/workerFocusStore";
import type {
  Pane,
  ThemeConfig,
  FontConfig,
  TerminalConfigSettings,
} from "../../lib/types";
import { UsageBubble } from "../Usage/UsageBubble";

interface TabContentProps {
  pane: Pane | undefined;
  isFocused: boolean;
  onFocusPane: (paneId: string) => void;
  onPtySpawned: (paneId: string, ptyId: string) => void;
  theme?: ThemeConfig;
  fontConfig?: FontConfig;
  terminalConfig?: TerminalConfigSettings;
  /** Per-provider usage-tracking toggle, resolved once by the parent. */
  usageEnabledByProvider?: Record<string, boolean>;
}

/**
 * Renders the single pane that lives inside one tab. Split layouts and the
 * browser pane variant were removed — every tab now has exactly one terminal,
 * so this is just a thin wrapper that handles focus pulse + materialize
 * animation on mount.
 */
export function TabContent({
  pane,
  isFocused,
  onFocusPane,
  onPtySpawned,
  theme,
  fontConfig,
  terminalConfig,
  usageEnabledByProvider,
}: TabContentProps) {
  if (!pane) return null;
  if (!isPaneVisible(pane)) return null;

  const usageProvider = pane.agent_type ?? null;

  return (
    <PaneWrapper paneId={pane.id} isFocused={isFocused}>
      <div className="relative w-full h-full">
        <TerminalPane
          key={pane.id}
          paneId={pane.id}
          ptyId={pane.pty_id}
          isFocused={isFocused}
          hasSplits={false}
          onFocus={onFocusPane}
          onPtySpawned={onPtySpawned}
          theme={theme}
          fontConfig={fontConfig}
          terminalConfig={terminalConfig}
        />
      </div>
      {usageProvider && (
        <UsageBubble
          provider={usageProvider}
          trackingEnabled={usageEnabledByProvider?.[usageProvider] === true}
        />
      )}
    </PaneWrapper>
  );
}

interface PaneWrapperProps {
  paneId: string;
  isFocused: boolean;
  children: React.ReactNode;
}

function PaneWrapper({ paneId, isFocused, children }: PaneWrapperProps) {
  const [focusPulse, setFocusPulse] = useState(false);
  const prevFocused = useRef(isFocused);

  // Tiny pulse when focus enters this pane — used to be tied to split layouts
  // but the visual cue is still nice when switching tabs.
  useEffect(() => {
    if (isFocused && !prevFocused.current) {
      setFocusPulse(true);
      const t = setTimeout(() => setFocusPulse(false), 400);
      return () => clearTimeout(t);
    }
    prevFocused.current = isFocused;
  }, [isFocused]);

  return (
    <div
      data-pane-id={paneId}
      className={`w-full h-full relative group/pane ${focusPulse ? "pane-focus-pulse" : ""}`}
    >
      {children}
    </div>
  );
}
