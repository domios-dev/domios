import { TerminalPane } from "./TerminalPane";
import { PaneSessionChat } from "./PaneSessionChat";
import { PaneTopBar } from "./PaneTopBar";
import { isPaneVisible } from "../MemoryPanel/workerFocusStore";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { FirstRunGeneralPicker, isFirstRunGeneralPane } from "../AgentPicker";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import type {
  ApiConfig,
  Pane,
  ThemeConfig,
  FontConfig,
  TerminalConfigSettings,
} from "../../lib/types";
import { usageTrackingEnabled } from "./usageTracking";
import { paneSessionActionView } from "./paneSessionActions";

interface TabContentProps {
  pane: Pane | undefined;
  isFocused: boolean;
  onFocusPane: (paneId: string) => void;
  onPtySpawned: (paneId: string, ptyId: string) => void;
  theme: ThemeConfig;
  fontConfig?: FontConfig;
  terminalConfig?: TerminalConfigSettings;
  usageEnabledByProvider?: Record<string, boolean | undefined>;
  api?: ApiConfig;
  workspaceCwd?: string | null;
  uiFontScale?: number;
  isVisible: boolean;
  chatFirst: boolean;
  chatDefaultPrompted: boolean;
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
  isVisible,
  chatFirst,
  chatDefaultPrompted,
  api,
}: TabContentProps) {
  const descriptor = useAgentDescriptor(pane?.agent_type);
  const { providers } = useAgentProviders();

  if (!pane) return null;
  if (!isPaneVisible(pane)) return null;

  if (isFirstRunGeneralPane(pane)) {
    return (
      <PaneWrapper paneId={pane.id}>
        <FirstRunGeneralPicker providers={providers} />
      </PaneWrapper>
    );
  }

  const chatBacking = pane.backing?.kind === "chat_backend" ? pane.backing : null;
  const isChatBackend = chatBacking !== null;
  const sessionActions = paneSessionActionView(pane);
  const showSwitch = !isChatBackend && descriptor?.supports_chat_ui === true;
  const usageProvider = !isChatBackend ? (pane.agent_type ?? null) : null;
  const usageVisible = usageTrackingEnabled(
    descriptor?.settings_schema ?? [],
    usageProvider ? usageEnabledByProvider?.[usageProvider] : false,
  );
  const cwd = pane.cwd ?? workspaceCwd ?? null;

  const body = isChatBackend ? (
    <PaneSessionChat paneId={pane.id} api={api} chatBackend uiFontScale={uiFontScale} isVisible={isVisible} />
  ) : (
    <TerminalPane
      key={pane.id}
      paneId={pane.id}
      ptyId={pane.pty_id}
      isFocused={isFocused}
      isVisible={isVisible}
      hasSplits={false}
      onFocus={onFocusPane}
      onPtySpawned={onPtySpawned}
      theme={theme}
      fontConfig={fontConfig}
      terminalConfig={terminalConfig}
      uiFontScale={uiFontScale}
      api={api}
    />
  );

  return (
    <PaneWrapper paneId={pane.id}>
      <div className="flex h-full w-full flex-col">
        <PaneTopBar
          paneId={pane.id}
          cwd={cwd}
          showSwitch={showSwitch}
          chatFirst={chatFirst}
          chatDefaultPrompted={chatDefaultPrompted}
          usageProvider={usageProvider}
          usageVisible={usageVisible}
          api={api}
          isWatcherPane={pane.watcher != null}
          watcherLastStatus={pane.watcher?.last_status ?? null}
          canReconnect={sessionActions.canReconnect}
          canRestartSession={sessionActions.canRestart}
          sessionId={sessionActions.sessionId}
          isActive={isVisible}
        />
        <div className="relative min-h-0 flex-1" style={{ background: theme.background }}>
          {body}
        </div>
      </div>
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
