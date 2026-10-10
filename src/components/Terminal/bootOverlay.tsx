import { tabStartingLabel } from "../Sidebar/tabAttention";
import { tabActivity, type TabActivity } from "../../lib/tabActivity";
import type { Tab } from "../../lib/types";
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
        <div
          className="absolute bottom-6 left-1/2 max-w-[min(34rem,calc(100%-2rem))] -translate-x-1/2 rounded-md border px-3 py-2 text-center"
          data-testid="tab-boot-failed"
          role="alert"
          style={{
            borderColor: "color-mix(in srgb, var(--ct-red, #f85149) 55%, transparent)",
            background: "color-mix(in srgb, var(--ct-bg, #0d1117) 94%, var(--ct-red, #f85149) 6%)",
            color: "var(--ct-fg, #c9d1d9)",
          }}
        >
          <div className="text-sm font-semibold">Launch failed</div>
          {view.reason && (
            <div className="mt-1 break-words text-xs opacity-75">
              {view.reason}
            </div>
          )}
          <div className="mt-2 flex flex-wrap justify-center gap-2">
            {view.canStartNewSession && (
              <button
                type="button"
                className="rounded-md bg-[var(--ct-accent)] px-2 py-1 text-xs font-medium text-[var(--ct-bg)]"
                aria-label="Start new session"
                disabled={startingNewSession}
                onClick={onStartNewSession}
              >
                {startingNewSession ? "Starting…" : "Start new session"}
              </button>
            )}
            {view.canRetry && (
              <button
                type="button"
                className="rounded-md border px-2 py-1 text-xs font-medium"
                aria-label="Retry agent launch"
                title="Retry agent launch"
                onMouseDown={(e) => e.preventDefault()}
                onClick={onRetry}
              >
                Retry
              </button>
            )}
            {view.canStartFreshAgent && (
              <button
                type="button"
                className="rounded-md border px-2 py-1 text-xs font-medium"
                aria-label="Start a fresh agent"
                onMouseDown={(e) => e.preventDefault()}
                onClick={onStartFreshAgent}
              >
                Start fresh agent
              </button>
            )}
            {view.canKeepShell && (
              <button
                type="button"
                className="rounded-md border px-2 py-1 text-xs font-medium"
                aria-label="Keep using this shell"
                onMouseDown={(e) => e.preventDefault()}
                onClick={onKeepShell}
              >
                Keep shell
              </button>
            )}
          </div>
          {actionError && <div role="alert" className="mt-1 text-xs">{actionError}</div>}
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
