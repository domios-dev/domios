import {isRemoteTaskTab} from "../../lib/remoteExecution";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { RestoreStatePanel } from "./RestoreStatePanel";
import { DeferredPresence } from "../shared/DeferredPresence";
import { restorePanelOwnsBody, restoreStageView } from "../../lib/restoreStageView";
import { useWakeOnOpen } from "./useWakeOnOpen";
import { TerminalTab } from "./TerminalTab";
import { useLayoutEffect, useRef } from "react";
import { useBodyPainted } from "./useBodyPainted";
import { consumePendingCommand, consumePendingCwd } from "../../lib/pendingCommands";
import { RestoredShell } from "./RestoredShell";
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
import { usageTrackingEnabled, tabUsageProvider } from "./usageTracking";
import { configureRemoteTabTransport } from "../../lib/remoteTabTransport";
import { useHeaderTabMenu } from "./useHeaderTabMenu";
import { SurfaceTabHost } from "./SurfaceTabHost";
import { RemoteTaskTab } from "./RemoteTaskTab";
import { surfaceBacking } from "../../lib/surfaceTab";
import { hasSleepingTranscript } from "../../lib/tabSleep";
import { usePanelInputBoundary } from "./usePanelInputBoundary";
import { useTabLock } from "../billing/useTabLock";
import { LockBadge, LockedOverlay } from "../billing/LockBadge";
import { openLockedPlans } from "../billing/freeLimitGate";

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
  if (tab?.remote_peer_id) configureRemoteTabTransport(api);
  const lock = useTabLock(tab);
  const wake = useWakeOnOpen(tab, isVisible && !lock.locked && Boolean(tab && isTabVisible(tab)));
  const descriptor = useAgentDescriptor(tab?.agent_type ?? (tab?.backing?.kind === "chat_backend" ? tab.backing.plugin_id : null));
  const { providers, loading } = useAgentProviders();
  const bodyRef = useRef<HTMLDivElement>(null);
  const bodyPainted = useBodyPainted(bodyRef, tab?.id ?? "");
  const headerTabMenu = useHeaderTabMenu(tab, workspaceId, descriptor);
  useLayoutEffect(() => {
    if (tab?.close_restore?.record_id) {
      consumePendingCommand(tab.id);
      consumePendingCwd(tab.id);
    }
  }, [tab?.id, tab?.close_restore?.record_id]);

  if (!tab) return null;
  if (!isTabVisible(tab)) return null;
  if (lock.locked) return <TabWrapper tabId={tab.id} isVisible={isVisible}>
    <LockBadge view={lock} />
    <LockedOverlay view={lock} onActivate={() => openLockedPlans(lock)} />
  </TabWrapper>;
  const restore = restoreStageView(tab, wake);
  const sleepingTranscript = hasSleepingTranscript(tab) && restore.phase !== "ready";
  const restoreOwnsBody = !sleepingTranscript && restorePanelOwnsBody(tab, wake, restore);
  const showRestore = restoreOwnsBody && !bodyPainted;
  const restoreInline = restoreOwnsBody && bodyPainted;
  const restorePanel = <RestoreStatePanel view={restore} />;

  if (isFirstRunGeneralTab(tab)) {
    return (
      <TabWrapper tabId={tab.id} isVisible={isVisible}>
        <FirstRunGeneralPicker providers={providers} home={workspaceId ? { workspaceId, tabId: tab.id } : undefined} />
      </TabWrapper>
    );
  }

  const surface = surfaceBacking(tab);
  if (surface) {
    return (
      <TabWrapper tabId={tab.id} isVisible={isVisible}>
        <div className="flex h-full flex-col">
          <div className="min-h-0 flex-1"><SurfaceTabHost tab={tab} backing={surface} api={api} isVisible={isVisible} /></div>
        </div>
      </TabWrapper>
    );
  }

  const chatBacking = tab.backing?.kind === "chat_backend" ? tab.backing : null;
  const isChatBackend = chatBacking !== null;
  const showSwitch = !isChatBackend && descriptor?.supports_chat_ui === true;
  const usageProvider = tabUsageProvider(tab.agent_type,isChatBackend,descriptor?.agent_runtime?.usage);
  const usageVisible = usageTrackingEnabled(
    descriptor?.settings_schema ?? [],
    usageProvider ? usageEnabledByProvider?.[usageProvider] : false,
  );
  // The catalog id the tab resolved at launch names its upstream group; the
  // live session model can be a bare leaf a group cannot be read from.
  const usageModelId =
    tab.agent_provider_resolved_model ?? tab.agent_session_model ?? tab.agent_requested_model ?? null;
  const cwd = tab.cwd ?? workspaceCwd ?? null;

  const body = tab.frozen && !sleepingTranscript && (tab.agent_type || isChatBackend) ? null : tab.close_restore?.stopped ? <RestoredShell tab={tab} /> : chatBacking && descriptor?.remote_task && isRemoteTaskTab(tab, descriptor.remote_task) ? (
    tab.remote_peer_id ? <div role="status">Remote task controls are unavailable through this mesh tab.</div> :
    <RemoteTaskTab key={chatBacking.session_id} tabId={tab.id} pluginId={chatBacking.plugin_id} sessionId={chatBacking.session_id} capability={descriptor.remote_task} />
  ) : isChatBackend ? (
    <TabSessionChat tabId={tab.id} api={api} chatBackend uiFontScale={uiFontScale} isVisible={isVisible} />
  ) : (
    <TerminalTab
      key={tab.id}
      wakingTranscript={sleepingTranscript}
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
    <TabWrapper tabId={tab.id} isVisible={isVisible}>
      <div className="flex h-full w-full flex-col">
        <TabTopBar
          tabId={tab.id}
          shellTab={tab}
          cwd={cwd}
          workspaceId={workspaceId}
          showSwitch={showSwitch}
          usageProvider={usageProvider}
          usageVisible={usageVisible}
          usageModelId={usageModelId}
          usageModelGroups={descriptor?.model_groups}
          isWatcherTab={tab.watcher != null}
          watcherLastStatus={tab.watcher?.last_status ?? null}
          tabMenu={headerTabMenu ?? undefined}
          isActive={isVisible}
        />
        {!isChatBackend && tab.agent_type && !descriptor && loading && (
          <div role="status" data-testid="provider-capability-state" className="px-2 py-1 text-xs">
            Loading agent capabilities…
          </div>
        )}
        <div className="relative flex min-h-0 flex-1 flex-col" style={{ background: theme.background }}>
          {sleepingTranscript && <div role="status" aria-live="polite" className="shrink-0 px-3 py-2 text-xs text-[var(--ct-muted-fg)]">
            {restore.phase === "failed" ? <InlineErrorNotice error={restore.detail} operation="resuming this session" /> : "Waking…"}
          </div>}
          {restoreInline && <div role="status" aria-live="polite" className="shrink-0 px-3 py-2 text-xs text-[var(--ct-muted-fg)]">
            {restore.phase === "failed" ? <InlineErrorNotice error={restore.detail} operation="resuming this session" /> : restore.steps[Math.max(0, restore.current)]?.label ?? restore.label}
          </div>}
          <div ref={bodyRef} className="min-h-0 flex-1" inert={showRestore || undefined} aria-hidden={showRestore || undefined}>{body}</div>
          <DeferredPresence present={showRestore && isVisible} className="absolute inset-0 z-20">{restorePanel}</DeferredPresence>
        </div>
      </div>
    </TabWrapper>
  );
}

interface TabWrapperProps {
  tabId: string;
  isVisible: boolean;
  children: React.ReactNode;
}

function TabWrapper({ tabId, isVisible, children }: TabWrapperProps) {
  const ref = usePanelInputBoundary(isVisible);
  return (
    <div
      ref={ref}
      data-tab-id={tabId}
      inert={!isVisible || undefined}
      aria-hidden={!isVisible || undefined}
      className="w-full h-full relative group/Tab"
      style={{ outline: "none", boxShadow: "none", border: "none", background: "transparent", pointerEvents: isVisible ? undefined : "none" }}
    >
      {children}
    </div>
  );
}
