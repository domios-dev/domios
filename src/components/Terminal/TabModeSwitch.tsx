import { useEffect, useState, useCallback } from "react";
import { MessageSquare, Terminal } from "lucide-react";
import { Segmented, type SegmentedOption } from "../shared/Segmented";
import { emitTabUiMode, type TabUiMode } from "../../lib/chatUiMode";
import {
  listenCurrentTabUiMode,
  resolveInitialTabUiMode,
} from "../../lib/clientTabUiMode";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { tabFieldsEqual, selectTab } from "../../store/multiplexer/selectors";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";

const OPTIONS: readonly SegmentedOption<TabUiMode>[] = [
  { value: "chat", label: "Chat", icon: <MessageSquare size={13} aria-hidden="true" /> },
  { value: "terminal", label: "Terminal", icon: <Terminal size={13} aria-hidden="true" /> },
];

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

  return (
    <Segmented
      value={mode}
      options={OPTIONS}
      height={26}
      iconOnly
      aria-label="View"
      data-testid="tab-mode-switch"
      data-tab-id={tabId}
      className="ct-tab-view-switch"
      onChange={(next) => {
        setMode(next);
        emitTabUiMode(tabId, next);
      }}
    />
  );
}
