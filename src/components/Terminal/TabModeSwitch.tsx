import { useEffect, useState, useCallback, type KeyboardEvent } from "react";
import { MessageSquare, Terminal } from "lucide-react";
import { emitTabUiMode, type TabUiMode } from "../../lib/chatUiMode";
import {
  listenCurrentTabUiMode,
  resolveInitialTabUiMode,
} from "../../lib/clientTabUiMode";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { tabFieldsEqual, selectTab } from "../../store/multiplexer/selectors";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";

const options = [
  { mode: "chat", label: "Chat", Icon: MessageSquare },
  { mode: "terminal", label: "Terminal", Icon: Terminal },
] as const;

export function TabModeSwitch({
  tabId,
  chatPty = true,
}: {
  tabId: string;
  chatPty?: boolean;
}) {
  const tabSelector = useCallback((s: Parameters<typeof selectTab>[0]) => selectTab(s, tabId), [tabId]);
  const tab = useMuxSelector(tabSelector, tabFieldsEqual);
  const descriptor = useAgentDescriptor(tab?.agent_type);
  const chatCapable =
    tab?.backing?.kind === "chat_backend"
      ? true
      : !tab?.agent_type
        ? false
        : descriptor
          ? descriptor.supports_chat_ui === true
          : true;

  const [mode, setMode] = useState<TabUiMode>(() =>
    resolveInitialTabUiMode(tabId, {
      chatCapable,
    }),
  );
  useEffect(() => listenCurrentTabUiMode(tabId, setMode), [tabId]);

  if (!chatPty) return null;

  const select = (next: TabUiMode) => {
    setMode(next);
    emitTabUiMode(tabId, next);
  };
  const onArrowKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.code)) return;
    const next = mode === "chat" ? "terminal" : "chat";
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.querySelector<HTMLButtonElement>(`[data-value="${next}"]`)?.focus();
    select(next);
  };

  return (
    <div
      role="radiogroup"
      aria-label="View"
      data-testid="tab-mode-switch"
      data-tab-id={tabId}
      className="ct-tab-view-switch"
      onKeyDown={onArrowKey}
    >
      {options.map(({ mode: value, label, Icon }) => (
        <button
          key={value}
          type="button"
          role="radio"
          aria-label={label}
          aria-checked={mode === value}
          tabIndex={mode === value ? 0 : -1}
          title={label}
          data-value={value}
          onMouseDown={(event) => event.preventDefault()}
          onClick={(event) => {
            const button = event.currentTarget;
            if (button.parentElement?.contains(document.activeElement)) button.focus();
            select(value);
          }}
        >
          <Icon size={13} aria-hidden="true" />
        </button>
      ))}
    </div>
  );
}
