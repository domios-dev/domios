import { tabStartingLabel } from "../Sidebar/tabAttention";
import { tabActivity } from "../../lib/tabActivity";
import { useCallback } from "react";
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
    return (
      <div
        className="mx-auto w-full max-w-sm p-4 text-center"
        data-testid="chat-transcript-unavailable"
        role="status"
        aria-label="Chat transcript unavailable"
      >
        <div className="text-sm font-semibold" style={{ color: "var(--ct-fg, #c9d1d9)" }}>
          Chat transcript unavailable
        </div>
        <div className="mt-1 text-xs" style={{ color: "var(--ct-muted-fg, #8b949e)" }}>
          {UNAVAILABLE_COPY[view.reason]}
        </div>
      </div>
    );
  }
  if (view.kind !== "starting") return null;

  return (
    <DeferredLoader state="present" className="chat-launch-overlay flex items-center justify-center p-4" data-testid="chat-setup-state" onKeyDown={handleKeyDown}>
      <TabLoadingIndicator label={tab ? tabStartingLabel(tab) : `${providerName} starting`} iconSvg={providerIconSvg} iconColor={providerIconColor} iconHint={providerIconHint} deferred={false} />
    </DeferredLoader>
  );
}
