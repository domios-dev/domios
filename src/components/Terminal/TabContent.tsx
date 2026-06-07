import { useEffect, useRef, useState } from "react";
import { TerminalPane } from "./TerminalPane";
import { isPaneVisible } from "../MemoryPanel/workerFocusStore";
import type {
  ApiConfig,
  Pane,
  ThemeConfig,
  FontConfig,
  TerminalConfigSettings,
} from "../../lib/types";
import { UsageBubble } from "../Usage/UsageBubble";
import { GitBubble } from "../GitPanel/GitBubble";
import type { GitInfo } from "../../hooks/useGitInfo";

interface TabContentProps {
  pane: Pane | undefined;
  isFocused: boolean;
  onFocusPane: (paneId: string) => void;
  onPtySpawned: (paneId: string, ptyId: string) => void;
  theme?: ThemeConfig;
  fontConfig?: FontConfig;
  terminalConfig?: TerminalConfigSettings;
  usageEnabledByProvider?: Record<string, boolean>;
  gitInfo?: GitInfo | null;
  onOpenGit?: (repoRoot: string) => void;
  api?: ApiConfig;
  workspaceCwd?: string | null;
}

export function TabContent({
  pane,
  isFocused,
  onFocusPane,
  onPtySpawned,
  theme,
  fontConfig,
  terminalConfig,
  usageEnabledByProvider,
  gitInfo,
  onOpenGit,
  api,
  workspaceCwd,
}: TabContentProps) {
  if (!pane) return null;
  if (!isPaneVisible(pane)) return null;

  const usageProvider = pane.agent_type ?? null;
  const usageVisible = !!usageProvider && usageEnabledByProvider?.[usageProvider] === true;

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
      {isFocused && onOpenGit && api && (
        <GitBubble
          info={gitInfo ?? null}
          cwd={pane.cwd ?? workspaceCwd ?? null}
          api={api}
          onClick={onOpenGit}
        />
      )}
      {usageProvider && (
        <UsageBubble
          provider={usageProvider}
          trackingEnabled={usageVisible}
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
