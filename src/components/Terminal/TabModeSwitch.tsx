import { useEffect, useState, useCallback } from "react";
import { Segmented, type SegmentedOption } from "../shared/Segmented";
import { emitTabUiMode, type TabUiMode } from "../../lib/chatUiMode";
import {
  listenCurrentTabUiMode,
  resolveInitialTabUiMode,
} from "../../lib/clientTabUiMode";
import { useMuxSelector } from "../../store/multiplexer";
import { tabFieldsEqual, selectTab } from "../../store/multiplexer/selectors";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";

const TERMINAL: SegmentedOption<TabUiMode> = { value: "terminal", label: "Terminal" };
const CHAT: SegmentedOption<TabUiMode> = { value: "chat", label: "Chat" };

/** Terminal | Chat switch for a tab. Plan navigation lives in workspace actions
 *  and the above-composer task strip, not as a third tab mode. */
export function TabModeSwitch({
  tabId,
  height,
  chatPty = true,
}: {
  tabId: string;
  height?: number;
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
      data-testid="tab-mode-switch"
      data-tab-id={tabId}
      aria-label="Tab view"
      value={mode}
      options={[CHAT, TERMINAL]}
      height={height}
      onChange={(next) => {
        setMode(next);
        emitTabUiMode(tabId, next);
      }}
    />
  );
}
