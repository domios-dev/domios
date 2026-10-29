import {isRemoteTaskTab} from "../../lib/remoteExecution";
import { BusyState } from "../shared/BusyState";
import { transcriptWindowKey, useTranscriptWindow } from "./transcriptStore";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { RestoreStatePanel } from "./RestoreStatePanel";
import { DeferredPresence } from "../shared/DeferredPresence";
import { restorePanelOwnsBody, restoreStageView, transcriptLoadingStageView } from "../../lib/restoreStageView";
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
  const { providers, loading, error: providersError } = useAgentProviders();
  const sessionId = tab?.agent_session_id || (tab?.backing?.kind === "chat_backend" ? tab.backing.session_id : null);
  const cached = useTranscriptWindow(transcriptWindowKey(tab?.id ?? "", sessionId ?? null)).transcript;
  const hasCachedTranscript = cached.authority?.sourceSessionId === sessionId
    && cached.authority?.sessionBindingNonce === (tab?.session_binding_nonce ?? null)
    && cached.segments.some(segment => segment.rows.length > 0);
  const restore = restoreStageView(tab ?? {}, wake);
  const bodyRef = useRef<HTMLDivElement>(null);
  const bodyPainted = useBodyPainted(bodyRef, tab?.id ?? "");
  const initialBody = useRef({ tabId: tab?.id, waiting: Boolean(tab?.frozen && !tab.slept_at_ms && (tab.agent_type || tab.backing?.kind === "chat_backend")) });
  if (initialBody.current.tabId !== tab?.id) initialBody.current = {
    tabId: tab?.id, waiting: Boolean(tab?.frozen && !tab.slept_at_ms && (tab.agent_type || tab.backing?.kind === "chat_backend")),
  };
  if (bodyPainted) initialBody.current.waiting = false;
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
  const sleepingTranscript = (hasSleepingTranscript(tab) || hasCachedTranscript) && restore.phase !== "ready";
  const restoreOwnsBody = !sleepingTranscript && !hasCachedTranscript && (restorePanelOwnsBody(tab, wake, restore) || initialBody.current.waiting);
  const showRestore = restoreOwnsBody && !bodyPainted;
  const restoreInline = restoreOwnsBody && bodyPainted;
  const restorePanel = <RestoreStatePanel view={initialBody.current.waiting && restore.phase === "ready" ? transcriptLoadingStageView(tab) : restore} />;

  if (isFirstRunGeneralTab(tab)) {
    return (
      <TabWrapper tabId={tab.id} isVisible={isVisible}>
        <FirstRunGeneralPicker providers={providers} providersLoading={loading} providersError={providersError} home={workspaceId ? { workspaceId, tabId: tab.id } : undefined} />
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

  const body = tab.frozen && !sleepingTranscript && !bodyPainted && (tab.agent_type || isChatBackend) ? null : tab.close_restore?.stopped ? <RestoredShell tab={tab} /> : chatBacking && descriptor?.remote_task && isRemoteTaskTab(tab, descriptor.remote_task) ? (
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
          <DeferredPresence present={isVisible && (sleepingTranscript || restoreInline || hasCachedTranscript) && (restore.phase !== "ready" || wake.pending || (hasCachedTranscript && !bodyPainted))}
            className="pointer-events-none absolute inset-x-0 top-2 z-30 flex justify-center">
            {restore.phase === "failed"
              ? <div className="pointer-events-auto"><InlineErrorNotice error={restore.detail} operation="resuming this session" /></div>
              : <BusyState inline label="Restoring…" phase="work" className="ct-bar-pill" />}
          </DeferredPresence>
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
