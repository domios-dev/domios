import { TerminalPane } from "./TerminalPane";
import { PaneSessionChat } from "./PaneSessionChat";
import { isPaneVisible } from "../MemoryPanel/workerFocusStore";
import type {
  ApiConfig,
  Pane,
  ThemeConfig,
  FontConfig,
  TerminalConfigSettings,
} from "../../lib/types";
import { UsageBubble } from "../Usage/UsageBubble";
import { PluginBubbles } from "../Extensions/PluginBubbles";

interface TabContentProps {
  pane: Pane | undefined;
  isFocused: boolean;
  onFocusPane: (paneId: string) => void;
  onPtySpawned: (paneId: string, ptyId: string) => void;
  theme: ThemeConfig;
  fontConfig?: FontConfig;
  terminalConfig?: TerminalConfigSettings;
  usageEnabledByProvider?: Record<string, boolean>;
  api?: ApiConfig;
  workspaceCwd?: string | null;
  uiFontScale?: number;
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
  workspaceCwd,
  uiFontScale,
}: TabContentProps) {
  if (!pane) return null;
  if (!isPaneVisible(pane)) return null;

  // A chatBackend pane has no PTY — render the chat UI directly and never mount
  // TerminalPane, so its xterm/usePtyConnection hooks never run (Appendix B1.1#6).
  if (pane.backing?.kind === "chat_backend") {
    return (
      <PaneWrapper paneId={pane.id}>
        <div className="relative w-full h-full" style={{ background: theme.background }}>
          <PaneSessionChat paneId={pane.id} chatBackend uiFontScale={uiFontScale} />
        </div>
        <PluginBubbles cwd={pane.cwd ?? workspaceCwd ?? null} paneId={pane.id} />
      </PaneWrapper>
    );
  }

  const usageProvider = pane.agent_type ?? null;
  const usageVisible = !!usageProvider && usageEnabledByProvider?.[usageProvider] === true;

  return (
    <PaneWrapper paneId={pane.id}>
      <div className="relative w-full h-full" style={{ background: theme.background }}>
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
          uiFontScale={uiFontScale}
        />
      </div>
      {/* Tab chrome, not focus chrome — the tab is only visible when active. */}
      {usageProvider && (
        <UsageBubble
          provider={usageProvider}
          trackingEnabled={usageVisible}
        />
      )}
      {/* Plugin-contributed status pills (Git, …) — next to the usage stats. */}
      <PluginBubbles cwd={pane.cwd ?? workspaceCwd ?? null} paneId={pane.id} />
    </PaneWrapper>
  );
}

interface PaneWrapperProps {
  paneId: string;
  children: React.ReactNode;
}

function PaneWrapper({ paneId, children }: PaneWrapperProps) {
  return (
    <div
      data-pane-id={paneId}
      className="w-full h-full relative group/pane"
      style={{ outline: "none", boxShadow: "none", border: "none", background: "transparent" }}
    >
      {children}
    </div>
  );
}
