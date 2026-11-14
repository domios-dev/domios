import { useCallback, type ReactNode } from "react";
import { levelIcon } from "../Notifications/levelIcon";
import { RestoreStatePanel } from "./RestoreStatePanel";
import { restoreStageView, transcriptLoadingStageView } from "../../lib/restoreStageView";
import { DeferredPresence } from "../shared/DeferredPresence";
import type { Tab, Workspace } from "../../lib/types";
import { TabLaunchFailure } from "./TabLaunchFailure";
import type { ChatBodyView } from "./chatBodyVisibility";
import type { ChatUnavailableReason } from "./chatSessionAuthority";
import type { LockedView } from "../billing/freeLimitModel";
import { LockedOverlay } from "../billing/LockBadge";
import { openLockedPlans } from "../billing/freeLimitGate";
import { useTabRecovery } from "./useTabRecovery";
import { describeRuntimeConnectionError } from "../../lib/describeError";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { runtimeSessionLoadRefused } from "../../lib/agentRuntimeState";
import { tabActivity } from "../../lib/tabActivity";
import { isReadOnlyTab } from "../../lib/tabInputPolicy";
import { chatLaunchFailureDescription } from "./chatSetupModel";

export { chatSetupView } from "./chatSetupModel";

const UNAVAILABLE_COPY: Record<ChatUnavailableReason, string> = {
  no_transcript_source: "This agent has no readable chat transcript for the current session.",
  authority_mismatch: "The chat transcript does not belong to this tab's current session.",
};

interface ChatSetupStateProps {
  visible?: boolean;
  waitingMessages?: number;
  workspace?: Workspace;
  tab: Tab | null | undefined;
  view: ChatBodyView;
  providerName: string;
  providerIconSvg?: string | null;
  providerIconColor?: string | null;
  providerIconHint?: string | null;
  onEscape?: () => void;
  onRetry?: () => void;
  onOpenTerminal?: () => void;
  lock?: LockedView;
  /** Kept rows share the log with this state, so a launch failure takes a row instead of the whole body. */
  inLog?: boolean;
}

export function ChatSetupState({
  visible = true,
  waitingMessages = 0,
  workspace,
  tab,
  view,
  onEscape,
  onRetry,
  onOpenTerminal,
  lock,
  inLog = false,
}: ChatSetupStateProps) {
  const { restart: recovery, fresh, actions: launchActions, error: launchActionError } = useTabRecovery(tab?.id,
    view.kind === "launch_failed" && !isReadOnlyTab(tab) ? tabActivity(tab).failure?.recovery : null);
  const handleKeyDown = useCallback((event: React.KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    onEscape?.();
  }, [onEscape]);

  if (view.kind === "locked") {
    return lock ? <LockedOverlay view={lock} onActivate={() => openLockedPlans(lock)} /> : null;
  }
  const startUnbound = tab?.agent_runtime_state?.state === "degraded"
    && tab.agent_runtime_state.failureCode === "runtime_start_unbound";
  if (runtimeSessionLoadRefused(tab?.agent_runtime_state) || startUnbound) {
    const detail = tab?.agent_runtime_state?.reason ?? "";
    return <div className="relative z-20 mx-auto my-2 w-full max-w-2xl" data-testid="chat-launch-failed">
      <TabLaunchFailure tabId={tab?.id} workspace={workspace} reason={detail} inline
        description={{ title: startUnbound ? "The agent could not start a session" : "The agent no longer has this session", cause: "", raw: detail, actions: [] }}
        actions={[{ label: fresh.startingNewSession ? "Starting…" : "Start new session", ariaLabel: "Start new session",
          disabled: fresh.startingNewSession, onClick: () => void fresh.onStartNewSession() }]} />
      {fresh.actionError && <InlineErrorNotice error={fresh.actionError} operation="starting a new session" />}
    </div>;
  }
  if (view.kind === "attach_failed") {
    const failure = describeRuntimeConnectionError(view.reason);
    return <ChatStatusBanner title="Chat connection failed" reason={failure.title} details={failure.raw} testId="chat-recovery-error"
      action={<button type="button" data-testid="chat-retry" onClick={onRetry}>Retry</button>} />;
  }
  if (view.kind === "launch_failed") {
    return <div className={`${inLog ? "my-3" : "absolute inset-0 z-20 justify-center overflow-auto bg-[var(--ct-bg)]"} flex flex-col items-center gap-4`} data-testid="chat-launch-failed">
      <TabLaunchFailure tabId={tab?.id} workspace={workspace} cwd={tab?.cwd} reason={view.reason}
        description={chatLaunchFailureDescription(tab, view.reason)}
        actions={onOpenTerminal && tab?.backing?.kind !== "chat_backend"
          ? [{ label: "Open terminal", onClick: onOpenTerminal }, ...launchActions] : launchActions} />
      {launchActionError && <InlineErrorNotice error={launchActionError} operation="recovering this tab" />}
      {waitingMessages > 0 && <p role="status" className="text-xs text-[var(--ct-muted-fg)]">{waitingMessages} {waitingMessages === 1 ? "message is" : "messages are"} waiting for this tab to start.</p>}
    </div>;
  }
  if (view.kind === "failed") {
    return <ChatStatusBanner title="Chat transcript unavailable" reason={UNAVAILABLE_COPY[view.reason]} testId="chat-transcript-unavailable" />;
  }
  return (
    <DeferredPresence present={visible && view.kind === "starting"} className="chat-launch-overlay">
    <div className="h-full" data-testid="chat-setup-state" onKeyDown={handleKeyDown}>
      <RestoreStatePanel view={view.kind === "starting" ? transcriptLoadingStageView(tab ?? {}) : restoreStageView(tab ?? {})} />
      {tab?.boot_stage === "launch_binding_unconfirmed" && !tab.agent_booting && <div className="absolute inset-x-0 bottom-8 flex flex-col items-center gap-3">
        <button type="button" disabled={recovery.restarting} onClick={() => void recovery.restart()}>{recovery.restarting ? "Restarting…" : "Restart agent"}</button>
        {recovery.error && <InlineErrorNotice error={recovery.error} operation="restarting this agent" />}
      </div>}
    </div>
    </DeferredPresence>
  );
}

export function ChatStatusBanner({ title, reason, action, testId, loading = false, icon, details }: {
  title: string;
  reason: string | null;
  action?: ReactNode;
  testId?: string;
  loading?: boolean;
  icon?: ReactNode;
  details?: string | null;
}) {
  const Icon = levelIcon("failure");
  return (
    <div className="mx-auto my-2 flex w-full max-w-2xl items-center gap-3 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-surface)] px-3 py-2"
      data-testid={testId} role="status" aria-live="polite" aria-label={title} aria-busy={loading}>
      <span className="flex shrink-0 items-center">
        {icon ?? <Icon size={14} aria-hidden="true" style={{ color: "var(--ct-danger)" }} />}
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-medium" style={{ color: "var(--ct-fg, #c9d1d9)" }}>{title}</div>
        {reason && <div className="line-clamp-2 break-words text-xs" title={reason}
          style={{ color: "var(--ct-muted-fg, #8b949e)" }}>{reason}</div>}
        {details && <details className="mt-1 text-xs text-[var(--ct-muted-fg)]">
          <summary className="cursor-pointer">Details</summary>
          <pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap break-words">{details}</pre>
        </details>}
      </div>
      {action && <div className="shrink-0">{action}</div>}
    </div>
  );
}
