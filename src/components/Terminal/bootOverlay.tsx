import { ErrorNotice, InlineErrorNotice, type ErrorNoticeAction } from "../shared/ErrorNotice";
import { describeError } from "../../lib/describeError";
import { WorkspaceFolderError } from "../Sidebar/WorkspaceFolderError";
import { tabStartingLabel } from "../Sidebar/tabAttention";
import { tabActivity, type TabActivity } from "../../lib/tabActivity";
import type { Tab, Workspace } from "../../lib/types";
import { restoreFailurePresentation } from "../../lib/restoreFailurePresentation";
import { TabLoadingIndicator } from "../shared/TabLoadingIndicator";

export type TabBootOverlayView =
  | { kind: "none" }
  | { kind: "starting"; label: string; activity: TabActivity }
  | {
      kind: "failed";
      reason: string | null;
      canRetry: boolean;
      canStartFreshAgent: boolean;
      canStartNewSession: boolean;
      canKeepShell: boolean;
    };

export function tabBootOverlayView(
  tab: Tab | null | undefined,
  keepShell = false,
): TabBootOverlayView {
  const failure = restoreFailurePresentation(
    {
      bootStage: tab?.boot_stage,
      agentType: tab?.agent_type,
      protectedTab: tab?.protected === true,
    },
    tab?.boot_failure_reason,
    keepShell,
  );
  if (failure.view === "launch_failure") {
    return {
      kind: "failed",
      reason: failure.reason,
      canRetry: failure.actions.retry,
      canStartFreshAgent: failure.actions.startFreshAgent,
      canStartNewSession: failure.actions.startNewSession,
      canKeepShell: failure.actions.keepShell,
    };
  }
  const activity = tabActivity(tab);
  return activity.launching
    ? { kind: "starting", label: tab ? tabStartingLabel(tab) : activity.label, activity }
    : { kind: "none" };
}

interface TabBootOverlayProps {
  view: TabBootOverlayView;
  workspace?: Workspace;
  cwd?: string | null;
  shellReady: boolean;
  overlayVisible: boolean;
  onRetry: () => void;
  onStartFreshAgent: () => void;
  onStartNewSession: () => void;
  startingNewSession?: boolean;
  actionError?: string | null;
  onKeepShell: () => void;
}

export function TabBootOverlay({
  view,
  workspace,
  cwd,
  shellReady,
  overlayVisible,
  onRetry,
  onStartFreshAgent,
  onStartNewSession,
  startingNewSession = false,
  actionError,
  onKeepShell,
}: TabBootOverlayProps) {
  if (!overlayVisible && view.kind === "none") return null;

  return (
    <div
      className={`absolute inset-0 ${!shellReady && view.kind !== "failed" ? "terminal-loading-overlay" : ""}`}
      data-testid="terminal-loading-overlay"
      style={{
        background: "var(--ct-bg, #0d1117)",
        opacity: view.kind === "none" && shellReady ? 0 : 1,
        transition: "opacity 0.3s ease-out",
        zIndex: view.kind === "failed" ? 20 : undefined,
      }}
    >
      {view.kind === "failed" ? (
        <div className="absolute bottom-6 left-1/2 w-[min(34rem,calc(100%-2rem))] -translate-x-1/2" data-testid="tab-boot-failed">
          {workspace && describeError(view.reason).actions.includes("choose-folder")
            ? <WorkspaceFolderError workspace={workspace} rawError={view.reason} cwd={cwd} />
            : <ErrorNotice error={view.reason} operation="starting this tab" cwd={cwd} blocking actions={bootActions(view, {
              onRetry, onStartFreshAgent, onStartNewSession, onKeepShell, startingNewSession,
            })} />}
          {actionError && <InlineErrorNotice error={actionError} operation="starting a new session" blocking />}
        </div>
      ) : (
        (view.kind === "starting" || !shellReady && overlayVisible) && (
          <div className="absolute bottom-6 left-1/2 -translate-x-1/2">
            <TabLoadingIndicator label={view.kind === "starting" ? view.label : "Terminal starting"} />
          </div>
        )
      )}
    </div>
  );
}

function bootActions(view: Extract<TabBootOverlayView, { kind: "failed" }>, props: Pick<TabBootOverlayProps, "onRetry" | "onStartFreshAgent" | "onStartNewSession" | "onKeepShell" | "startingNewSession">): ErrorNoticeAction[] {
  const actions: ErrorNoticeAction[] = [];
  if (view.canStartNewSession) actions.push({ label: props.startingNewSession ? "Starting…" : "Start new session", ariaLabel: "Start new session", onClick: props.onStartNewSession, disabled: props.startingNewSession });
  if (view.canRetry) actions.push({ label: "Retry", ariaLabel: "Retry agent launch", onClick: props.onRetry });
  if (view.canStartFreshAgent) actions.push({ label: "Start fresh agent", ariaLabel: "Start a fresh agent", onClick: props.onStartFreshAgent });
  if (view.canKeepShell) actions.push({ label: "Keep shell", ariaLabel: "Keep using this shell", onClick: props.onKeepShell });
  return actions;
}
