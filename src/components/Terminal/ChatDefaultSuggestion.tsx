import { useEffect, useState } from "react";
import { X } from "lucide-react";
import {
  isChatDefaultEnabled,
  markChatDefaultPrompted,
  setDefaultNewAgentPaneToChat,
  type PaneUiMode,
} from "../../lib/chatUiMode";
import {
  getClientPaneUiMode,
  getCurrentPaneUiMode,
  listenCurrentPaneUiMode,
} from "../../lib/clientPaneUiMode";

/**
 * One-time inline nudge beside the mode switch the first time a pane enters chat:
 * accept makes chat the default view (and swaps the picker order); dismiss just
 * stops asking. Both write `chat_ui_default_prompted`, so it never reappears.
 * `prompted`/`isDefault` arrive via props from App's single useConfig — never a
 * per-pane useConfig (that multiplied get_config by pane count, freezing switches).
 */
export function ChatDefaultSuggestion({
  paneId,
  height,
  prompted,
  isDefault,
}: {
  paneId: string;
  height?: number;
  prompted: boolean;
  isDefault: boolean;
}) {
  const [mode, setMode] = useState<PaneUiMode>(
    () => getCurrentPaneUiMode(paneId) ?? getClientPaneUiMode(paneId) ?? "terminal",
  );
  useEffect(() => listenCurrentPaneUiMode(paneId, setMode), [paneId]);
  const defaultEnabled = isDefault || isChatDefaultEnabled();

  if (mode !== "chat" || prompted || defaultEnabled) return null;

  return (
    <div
      data-testid="chat-default-suggestion"
      className="relative z-[3] flex shrink-0 items-center gap-0.5 rounded-full pl-3 pr-1 pointer-events-auto"
      style={{
        height,
        background: "color-mix(in srgb, var(--ct-accent) 12%, transparent)",
        border: "1px solid color-mix(in srgb, var(--ct-accent) 30%, transparent)",
        color: "var(--ct-fg)",
      }}
    >
      <button
        type="button"
        data-testid="chat-default-accept"
        onClick={() => void setDefaultNewAgentPaneToChat(true)}
        className="relative z-[1] flex h-full items-center text-[11px] font-medium leading-none hover:underline"
        title="Open new agent tabs in chat by default"
      >
        Use chat as default view?
      </button>
      <button
        type="button"
        data-testid="chat-default-dismiss"
        onClick={() => void markChatDefaultPrompted()}
        aria-label="Dismiss"
        title="Dismiss"
        className="relative z-[1] flex h-5 w-5 items-center justify-center rounded-full text-[var(--ct-muted-fg)] transition-colors hover:text-[var(--ct-fg)]"
      >
        <X size={12} strokeWidth={2.2} />
      </button>
    </div>
  );
}
