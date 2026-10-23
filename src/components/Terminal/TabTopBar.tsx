import { createNoticePublisher } from "../../lib/notices";
import { describeError } from "../../lib/describeError";
import { useMultiplexer } from "../../store/multiplexer";
import { tabActivity } from "../../lib/tabActivity";
import { ComposerContextMeter } from "./ComposerContextMeter";
import { DocumentsButton } from "./DocumentsPanel";
import { ChangesButton } from "./ChangesPanel";
import { planSidePanelStore } from "../Canvas/planSidePanelStore";
import { ShellPurposeIcon, ShellStatus } from "../shared/ShellStatus";
import { isUtilityShell } from "../../lib/shellCommandState";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ChevronDown } from "lucide-react";
import { TabModeSwitch } from "./TabModeSwitch";
import { UsageBubble } from "../Usage/UsageBubble";
import { PluginBubbles } from "../Extensions/PluginBubbles";
import type { ModelGroup, Tab } from "../../lib/types";
import { TAB_BAR_CONTROL_H } from "./tabBarConstants";
import { WatcherStatusBadge } from "./WatcherStatusBadge";
import { TabActionsMenu } from "./TabActionsMenu";
import { matchesBinding } from "../../lib/keybindings";
import { usePluginTabMenu } from "../../lib/pluginTabMenu";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { reconnectTabSession, restartTabSession, tabSessionActionView } from "./tabSessionActions";
import { buildTabMenu, type TabMenuAction, type TabMenuContext } from "./tabMenuModel";
import { performTabMenuAction } from "./tabMenuActions";
import { useProviderSwitchDialog } from "../Sidebar/ProviderSwitchDialog";
import { useTelegramThreadMenu } from "./useTelegramThreadMenu";
import { ReceiverTargetMarks } from "./ReceiverTargetMark";
import { BlueprintOwnerCloseDialog } from "../Sidebar/BlueprintOwnerCloseDialog";
import { readWorkspaceBlueprints } from "../Automation/blueprintOwner";
import { runningBlueprintNamesForOwner } from "../Automation/blueprintOwnerModel";

/** Single height rhythm for every control in the bar (switch + pills). */
export { TAB_BAR_CONTROL_H };

export interface HeaderTabMenu {
  tab: Tab;
  context: TabMenuContext;
  onRename?: (title: string) => void;
  onClose?: () => void;
}

interface TabTopBarProps {
  tabId: string;
  shellTab?: Tab;
  cwd: string | null;
  /** Show the Terminal | Chat switch (chat-capable PTY tabs only). */
  showSwitch: boolean;
  /** Usage provider id, or null to omit the usage pill. */
  usageProvider?: string | null;
  usageVisible?: boolean;
  /** The tab's selected model + its provider-declared groups, so a group-spend
   *  capsule shows for the upstream the tab is actually running. */
  usageModelId?: string | null;
  usageModelGroups?: ModelGroup[];
  /** Watcher tabs surface health from `tab.watcher.last_status` in the header. */
  watcherLastStatus?: string | null;
  isWatcherTab?: boolean;
  isActive?: boolean;
  workspaceId?: string | null;
  tabMenu?: HeaderTabMenu;
}

function TabRenameField({ title, onDone }: { title: string; onDone: (title: string | null) => void }) {
  const [value, setValue] = useState(title);
  return (
    <input
      autoFocus
      aria-label={`Rename tab ${title}`}
      data-testid="tab-header-rename"
      value={value}
      onChange={(event) => setValue(event.target.value)}
      onBlur={() => {
        const trimmed = value.trim();
        onDone(trimmed && trimmed !== title ? trimmed : null);
      }}
      onKeyDown={(event) => {
        if (event.key !== "Enter" && event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        if (event.key === "Enter") event.currentTarget.blur();
        else onDone(null);
      }}
      className="min-w-0 rounded border border-[var(--ct-border)] bg-[var(--ct-bg)] px-1.5 text-[12px] text-[var(--ct-fg)] outline-none"
      style={{ height: TAB_BAR_CONTROL_H }}
    />
  );
}

/**
 * Tab header strip: mode switch on the left, status pills (usage + plugins) on
 * the right. Every item shares `TAB_BAR_CONTROL_H` — published to CSS-driven
 * children via the `--ct-tab-pill-h` custom property.
 */
export function TabTopBar({
  tabId,
  shellTab,
  cwd,
  showSwitch,
  usageProvider,
  usageVisible,
  usageModelId,
  usageModelGroups,
  watcherLastStatus,
  isWatcherTab = false,
  isActive = false,
  workspaceId,
  tabMenu,
}: TabTopBarProps) {
  useEffect(() => { if (isActive) planSidePanelStore.activateTab(tabId); }, [isActive, tabId]);
  const [reconnecting, setReconnecting] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [closeBlueprints, setCloseBlueprints] = useState<string[] | null>(null);
  const { upsertToast } = useMultiplexer();
  const menuTab = tabMenu?.tab ?? null;
  const { openProviderSwitch, providerSwitchDialog } = useProviderSwitchDialog(menuTab);
  const onRename = tabMenu?.onRename;
  const canReconnect = menuTab ? tabSessionActionView(menuTab).canReconnect : false;
  const pluginTabMenu = usePluginTabMenu();
  const telegramThread = useTelegramThreadMenu(tabId, menuTab !== null && !menuTab.remote_peer_id);

  const reportFailure = useCallback((error: unknown, operation: string, retry?: () => void | Promise<void>) => {
    const description = describeError(error, { operation });
    createNoticePublisher({ upsertToast })({ kind: "toast", id: `tab-header-action-failure-${tabId}`,
      title: description.title,
      detail: description.cause,
      severity: "Warning",
      tabId: tabId,
      workspaceId: workspaceId ?? null,
      actions: [
        ...(description.raw ? [{ label: "Copy details", run: async () => {
              try {
                await navigator.clipboard.writeText(description.raw);
              }
              catch {
                return;
              }
            } }] : []),
        ...(retry ? [{ label: "Retry", run: retry }] : []),
      ] });
  }, [tabId, upsertToast, workspaceId]);

  const runRetryable = useCallback(async (operation: string, action: () => Promise<unknown>): Promise<void> => {
    const run = async () => {
      try {
        await action();
      } catch (error) {
        reportFailure(error, operation, run);
      }
    };
    await run();
  }, [reportFailure]);

  const reconnect = useCallback(async () => {
    if (reconnecting) return;
    setReconnecting(true);
    try {
      await runRetryable("reconnecting this tab", () => reconnectTabSession(tabId));
    } catch (error) {
      reportFailure(error, "reconnecting this tab", () => reconnect());
    } finally {
      setReconnecting(false);
    }
  }, [tabId, reconnecting, runRetryable, reportFailure]);

  const restartSession = useCallback(async () => {
    if (restarting) return;
    setRestarting(true);
    try {
      await runRetryable("restarting this tab", () => restartTabSession(tabId));
    } catch (error) {
      reportFailure(error, "restarting this tab", () => restartSession());
    } finally {
      setRestarting(false);
    }
  }, [tabId, restarting, runRetryable, reportFailure]);

  useEffect(() => {
    if (!isActive || !canReconnect) return;
    const handleReloadShortcut = (event: KeyboardEvent) => {
      if (!matchesBinding(event, "Ctrl+R")) return;
      event.preventDefault();
      event.stopPropagation();
      void reconnect();
    };
    window.addEventListener("keydown", handleReloadShortcut, true);
    return () => window.removeEventListener("keydown", handleReloadShortcut, true);
  }, [canReconnect, isActive, reconnect]);

  const entries = tabMenu
    ? buildTabMenu({
      ...tabMenu.context,
      tab: tabMenu.tab,
      surface: "header",
      canRename: Boolean(tabMenu.onRename),
      canClose: Boolean(tabMenu.onClose),
      pluginTabMenu,
      telegramThread: telegramThread.state,
      busy: { reconnecting, restarting },
    })
    : [];

  const runMenuAction = (action: TabMenuAction) => {
    if (action.type === "reconnect") {
      void reconnect();
      return;
    }
    if (action.type === "restart") {
      void restartSession();
      return;
    }
    if (action.type === "close") {
      void (async () => {
        if (!tabMenu?.onClose) return;
        if (!workspaceId) {
          reportFailure(new Error("Could not read Blueprint close inventory: workspace unavailable."), "closing this tab");
          return;
        }
        try {
          const blueprints = await readWorkspaceBlueprints(workspaceId);
          const running = runningBlueprintNamesForOwner(blueprints, "tab", tabId);
          if (running.length > 0) setCloseBlueprints(running);
          else tabMenu.onClose?.();
        } catch (error) {
          reportFailure(error, "closing this tab", () => runMenuAction(action));
        }
      })();
      return;
    }
    performTabMenuAction(tabId, action, { sessionBindingNonce: menuTab?.session_binding_nonce, switchProvider: openProviderSwitch, rename: () => setRenaming(true), close: tabMenu?.onClose })
      .catch((error) => reportFailure(error, "updating this tab"));
  };

  // Bubbles that do not fit one row stay clipped behind a toggle instead of wrapping the bar onto a second line.
  const bubblesRef = useRef<HTMLDivElement>(null);
  const [hiddenBubbles, setHiddenBubbles] = useState(0);
  const [bubblesOpen, setBubblesOpen] = useState(false);
  useLayoutEffect(() => {
    const row = bubblesRef.current;
    if (!row) return;
    // Bubbles mount and resize on their own (plugins load late), so both child changes and child sizes re-measure.
    const sizes = new ResizeObserver(() => check());
    const check = () => {
      const top = row.getBoundingClientRect().top;
      setHiddenBubbles(Array.from(row.children).filter((child) => {
        if (!(child instanceof HTMLElement) || child.dataset.bubblesToggle) return false;
        const box = child.getBoundingClientRect();
        return box.width > 0 && box.top - top >= TAB_BAR_CONTROL_H;
      }).length);
    };
    const watch = () => {
      sizes.disconnect();
      sizes.observe(row);
      for (const child of Array.from(row.children)) sizes.observe(child);
      check();
    };
    const mutations = new MutationObserver(watch);
    mutations.observe(row, { childList: true, subtree: true, characterData: true });
    watch();
    return () => { sizes.disconnect(); mutations.disconnect(); };
  }, []);

  return (
    <div
      data-testid="tab-top-bar"
      className="ct-tab-top-bar flex min-w-0 shrink-0 flex-nowrap items-start justify-between gap-1.5 px-2 sm:gap-2"
      style={{
        ["--ct-tab-pill-h" as string]: `${TAB_BAR_CONTROL_H}px`,
        paddingTop: 6,
        paddingBottom: 6,
      }}
    >
      <div className="flex min-w-0 shrink items-center gap-1.5 sm:gap-2">
        {renaming && menuTab && onRename ? (
          <TabRenameField
            title={displayTitleForTab(menuTab)}
            onDone={(title) => {
              setRenaming(false);
              if (title) onRename(title);
            }}
          />
        ) : showSwitch && (
          <TabModeSwitch
            tabId={tabId}
            height={TAB_BAR_CONTROL_H}
            chatPty={showSwitch}
          />
        )}
      </div>
      <div className="ml-auto flex min-w-0 flex-1 items-start justify-end gap-1 sm:gap-1.5">
      <div
        ref={bubblesRef}
        className="flex min-w-0 flex-1 flex-wrap items-center justify-end gap-1 overflow-hidden sm:gap-1.5"
        style={bubblesOpen
          ? { background: "var(--ct-bg)", rowGap: 8, paddingBottom: 8, marginBottom: -8, boxShadow: "0 12px 12px -6px var(--ct-bg)", position: "relative", zIndex: 3 }
          : { maxHeight: TAB_BAR_CONTROL_H }}
      >
        {(hiddenBubbles > 0 || bubblesOpen) && (
          <button
            type="button"
            data-bubbles-toggle="true"
            className="ct-bar-pill inline-flex shrink-0 items-center justify-center gap-0.5 px-2 text-xs"
            style={{ height: TAB_BAR_CONTROL_H }}
            aria-expanded={bubblesOpen}
            aria-label={bubblesOpen ? "Show fewer" : `Show ${hiddenBubbles} more`}
            title={bubblesOpen ? "Show fewer" : `Show ${hiddenBubbles} more`}
            onClick={() => setBubblesOpen((open) => !open)}
          >
            {bubblesOpen ? <ChevronDown size={14} aria-hidden style={{ transform: "rotate(180deg)" }} /> : <>+{hiddenBubbles}<ChevronDown size={12} aria-hidden /></>}
          </button>
        )}
        {shellTab && isUtilityShell(shellTab) && <>
          <ShellPurposeIcon purpose={shellTab.purpose} icon={shellTab.icon} running={tabActivity(shellTab).active} />
          <ShellStatus tab={shellTab} />
        </>}
        {isWatcherTab && <WatcherStatusBadge lastStatus={watcherLastStatus} />}
        {/* Order is priority: the first row keeps tokens, then changes, then plugins. */}
        {usageProvider && (
          <UsageBubble
            provider={usageProvider}
            tabId={tabId}
            threadId={shellTab?.agent_session_id}
            trackingEnabled={!!usageVisible}
            modelId={usageModelId}
            modelGroups={usageModelGroups}
          />
        )}
        <ComposerContextMeter tabId={tabId} />
        <div className="ct-bar-pill-group">
          <DocumentsButton tabId={tabId} />
          <ChangesButton tabId={tabId} />
        </div>
        <ReceiverTargetMarks menu={pluginTabMenu} tabId={tabId} surface="header" />
        <PluginBubbles cwd={cwd} tabId={tabId} isActive={isActive} />
      </div>
        {entries.length > 0 && (
          <TabActionsMenu
            entries={entries}
            busy={reconnecting || restarting}
            onAction={runMenuAction}
            onOpen={telegramThread.refresh}
          />
        )}
      </div>
      {providerSwitchDialog}
      {closeBlueprints && tabMenu && <BlueprintOwnerCloseDialog
        owner={`Tab ${menuTab?.title ?? "Tab"}`}
        blueprints={closeBlueprints}
        onCancel={() => setCloseBlueprints(null)}
        onConfirm={() => { setCloseBlueprints(null); tabMenu.onClose?.(); }}
      />}
    </div>
  );
}
