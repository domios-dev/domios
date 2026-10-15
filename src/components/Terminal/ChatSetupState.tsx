import { tabStartingLabel } from "../Sidebar/tabAttention";
import { useCallback, type ReactNode } from "react";
import { levelIcon } from "../Notifications/levelIcon";
import { TabLoadingIndicator } from "../shared/TabLoadingIndicator";
import { DeferredLoader } from "../shared/DeferredLoader";
import type { Tab, Workspace } from "../../lib/types";
import { TabLaunchFailure } from "./TabLaunchFailure";
import type { ChatBodyView } from "./chatBodyVisibility";
import type { ChatUnavailableReason } from "./chatSessionAuthority";

export { chatSetupView } from "./chatSetupModel";

const UNAVAILABLE_COPY: Record<ChatUnavailableReason, string> = {
  no_transcript_source: "This agent has no readable chat transcript for the current session.",
  authority_mismatch: "The chat transcript does not belong to this tab's current session.",
};

interface ChatSetupStateProps {
  waitingMessages?: number;
  workspace?: Workspace;
  tab: Tab | null | undefined;
  view: ChatBodyView;
  providerName: string;
  providerIconSvg?: string | null;
  providerIconColor?: string | null;
  providerIconHint?: string | null;
  onEscape?: () => void;
}

export function ChatSetupState({
  waitingMessages = 0,
  workspace,
  tab,
  view,
  providerName,
  providerIconSvg,
  providerIconColor,
  providerIconHint,
  onEscape,
}: ChatSetupStateProps) {
  const handleKeyDown = useCallback((event: React.KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    onEscape?.();
  }, [onEscape]);

  if (view.kind === "launch_failed") {
    return <div className="absolute inset-0 z-20 flex flex-col items-center justify-center gap-4 bg-[var(--ct-bg)] p-6" data-testid="chat-launch-failed">
      <TabLaunchFailure tabId={tab?.id} workspace={workspace} cwd={tab?.cwd} reason={view.reason} />
      {waitingMessages > 0 && <p role="status" className="text-xs text-[var(--ct-muted-fg)]">{waitingMessages} {waitingMessages === 1 ? "message is" : "messages are"} waiting for this tab to start.</p>}
    </div>;
  }
  if (view.kind === "failed") {
    return <ChatStatusBanner title="Chat transcript unavailable" reason={UNAVAILABLE_COPY[view.reason]} testId="chat-transcript-unavailable" />;
  }
  if (view.kind !== "starting") return null;

  return (
    <DeferredLoader state="present" className="chat-launch-overlay flex items-center justify-center p-4" data-testid="chat-setup-state" onKeyDown={handleKeyDown}>
      <TabLoadingIndicator label={tab ? tabStartingLabel(tab) : `${providerName} starting`} iconSvg={providerIconSvg} iconColor={providerIconColor} iconHint={providerIconHint} deferred={false} />
    </DeferredLoader>
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
