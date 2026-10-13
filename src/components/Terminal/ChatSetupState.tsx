import { tabStartingLabel } from "../Sidebar/tabAttention";
import { tabActivity } from "../../lib/tabActivity";
import { useCallback, type ReactNode } from "react";
import { levelIcon } from "../Notifications/levelIcon";
import { TabLoadingIndicator } from "../shared/TabLoadingIndicator";
import { DeferredLoader } from "../shared/DeferredLoader";
import type { Tab } from "../../lib/types";
import type { ChatBodyView } from "./chatBodyVisibility";
import type { ChatUnavailableReason } from "./chatSessionAuthority";

export function chatSetupView(
  tab: Tab | null | undefined,
  chatSessionAttached = false,
): ChatBodyView {
  if (chatSessionAttached) return { kind: "none" };
  const activity = tabActivity(tab);
  return activity.launching || activity.kind === "failed"
    ? { kind: "starting", label: activity.label }
    : { kind: "none" };
}

const UNAVAILABLE_COPY: Record<ChatUnavailableReason, string> = {
  no_transcript_source: "This agent has no readable chat transcript for the current session.",
  authority_mismatch: "The chat transcript does not belong to this tab's current session.",
};

interface ChatSetupStateProps {
  tab: Tab | null | undefined;
  view: ChatBodyView;
  providerName: string;
  providerIconSvg?: string | null;
  providerIconColor?: string | null;
  providerIconHint?: string | null;
  onEscape?: () => void;
}

export function ChatSetupState({
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

export function ChatStatusBanner({ title, reason, action, testId, loading = false, icon }: {
  title: string;
  reason: string | null;
  action?: ReactNode;
  testId?: string;
  loading?: boolean;
  icon?: ReactNode;
}) {
  const Icon = levelIcon("failure");
  return (
    <div className="mx-auto w-full max-w-sm p-4 text-center" data-testid={testId}
      role="status" aria-live="polite" aria-label={title} aria-busy={loading}>
      <div className="flex items-center justify-center gap-2 text-sm font-semibold" style={{ color: "var(--ct-fg, #c9d1d9)" }}>
        {icon ?? <Icon size={14} aria-hidden="true" style={{ color: "var(--ct-danger)" }} />}
        {title}
      </div>
      {reason && <div className="mt-1 break-words text-xs" style={{ color: "var(--ct-muted-fg, #8b949e)" }}>{reason}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}
