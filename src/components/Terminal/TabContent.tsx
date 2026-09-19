import { TerminalTab } from "./TerminalTab";
import { TabSessionChat } from "./TabSessionChat";
import { TabTopBar } from "./TabTopBar";
import { isTabVisible } from "../MemoryPanel/workerFocusStore";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { FirstRunGeneralPicker, isFirstRunGeneralTab } from "../AgentPicker";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import type {
  ApiConfig,
  Tab,
  ThemeConfig,
  FontConfig,
  TerminalConfigSettings,
} from "../../lib/types";
import { usageTrackingEnabled } from "./usageTracking";
import { configureRemoteTabTransport } from "../../lib/remoteTabTransport";
import { useHeaderTabMenu } from "./useHeaderTabMenu";

interface TabContentProps {
  tab: Tab | undefined;
  isFocused: boolean;
  onFocusTab: (tabId: string) => void;
  onPtySpawned: (tabId: string, ptyId: string) => void;
  theme: ThemeConfig;
  fontConfig?: FontConfig;
  terminalConfig?: TerminalConfigSettings;
  usageEnabledByProvider?: Record<string, boolean | undefined>;
  api?: ApiConfig;
  workspaceCwd?: string | null;
  workspaceId?: string | null;
  uiFontScale?: number;
  isVisible: boolean;
}

export function TabContent({
  tab,
  isFocused,
  onFocusTab,
  onPtySpawned,
  theme,
  fontConfig,
  terminalConfig,
  usageEnabledByProvider,
  workspaceCwd,
  workspaceId = null,
  uiFontScale,
  isVisible,
  api,
}: TabContentProps) {
  const descriptor = useAgentDescriptor(tab?.agent_type);
  const { providers } = useAgentProviders();
  const headerTabMenu = useHeaderTabMenu(tab, workspaceId, descriptor);

  if (!tab) return null;
  if (!isTabVisible(tab)) return null;
  if (tab.remote_peer_id) configureRemoteTabTransport(api);

  if (isFirstRunGeneralTab(tab)) {
    return (
      <TabWrapper tabId={tab.id}>
        <FirstRunGeneralPicker providers={providers} />
      </TabWrapper>
    );
  }

  const chatBacking = tab.backing?.kind === "chat_backend" ? tab.backing : null;
  const isChatBackend = chatBacking !== null;
  const showSwitch = !isChatBackend && descriptor?.supports_chat_ui === true;
  const usageProvider = !isChatBackend ? (tab.agent_type ?? null) : null;
  const usageVisible = usageTrackingEnabled(
    descriptor?.settings_schema ?? [],
    usageProvider ? usageEnabledByProvider?.[usageProvider] : false,
  );
  // The catalog id the tab resolved at launch names its upstream group; the
  // live session model can be a bare leaf a group cannot be read from.
  const usageModelId =
    tab.agent_provider_resolved_model ?? tab.agent_session_model ?? tab.agent_requested_model ?? null;
  const cwd = tab.cwd ?? workspaceCwd ?? null;

  const body = isChatBackend ? (
    <TabSessionChat tabId={tab.id} api={api} chatBackend uiFontScale={uiFontScale} isVisible={isVisible} />
  ) : (
    <TerminalTab
      key={tab.id}
      tabId={tab.id}
      ptyId={tab.pty_id}
      isFocused={isFocused}
      isVisible={isVisible}
      hasSplits={false}
      onFocus={onFocusTab}
      onPtySpawned={onPtySpawned}
      theme={theme}
      fontConfig={fontConfig}
      terminalConfig={terminalConfig}
      uiFontScale={uiFontScale}
      api={api}
    />
  );

  return (
    <TabWrapper tabId={tab.id}>
      <div className="flex h-full w-full flex-col">
        <TabTopBar
          tabId={tab.id}
          cwd={cwd}
          showSwitch={showSwitch}
          usageProvider={usageProvider}
          usageVisible={usageVisible}
          usageModelId={usageModelId}
          usageModelGroups={descriptor?.model_groups}
          api={api}
          isWatcherTab={tab.watcher != null}
          watcherLastStatus={tab.watcher?.last_status ?? null}
          tabMenu={headerTabMenu ?? undefined}
          isActive={isVisible}
        />
        <div className="relative min-h-0 flex-1" style={{ background: theme.background }}>
          {body}
        </div>
      </div>
    </TabWrapper>
  );
}

interface TabWrapperProps {
  tabId: string;
  children: React.ReactNode;
}

function TabWrapper({ tabId, children }: TabWrapperProps) {
  return (
    <div
      data-tab-id={tabId}
      className="w-full h-full relative group/Tab"
      style={{ outline: "none", boxShadow: "none", border: "none", background: "transparent" }}
    >
      {children}
    </div>
  );
}
