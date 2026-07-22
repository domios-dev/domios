import { useEffect, useState, useCallback } from "react";
import { Segmented, type SegmentedOption } from "../shared/Segmented";
import { emitPaneUiMode, isChatDefaultEnabled, type PaneUiMode } from "../../lib/chatUiMode";
import {
  listenCurrentPaneUiMode,
  listenPaneUiModeDefaultChanged,
  resolveInitialPaneUiMode,
} from "../../lib/clientPaneUiMode";
import { useMuxSelector } from "../../store/multiplexer";
import { paneFieldsEqual, selectPane } from "../../store/multiplexer/selectors";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";

const TERMINAL: SegmentedOption<PaneUiMode> = { value: "terminal", label: "Terminal" };
const CHAT: SegmentedOption<PaneUiMode> = { value: "chat", label: "Chat" };

/** Terminal | Chat switch for a pane. Plan navigation lives in workspace actions
 *  and the above-composer task strip, not as a third tab mode. */
export function PaneModeSwitch({
  paneId,
  height,
  chatFirst = false,
  chatPty = true,
}: {
  paneId: string;
  height?: number;
  chatFirst?: boolean;
  chatPty?: boolean;
}) {
  const paneSelector = useCallback((s: Parameters<typeof selectPane>[0]) => selectPane(s, paneId), [paneId]);
  const pane = useMuxSelector(paneSelector, paneFieldsEqual);
  const descriptor = useAgentDescriptor(pane?.agent_type);
  const chatCapable =
    pane?.backing?.kind === "chat_backend"
      ? true
      : !pane?.agent_type
        ? false
        : descriptor
          ? descriptor.supports_chat_ui === true
          : true;

  const [mode, setMode] = useState<PaneUiMode>(() =>
    resolveInitialPaneUiMode(paneId, {
      chatCapable,
      chatViewFlag: pane?.chat_view === true,
      chatDefault: isChatDefaultEnabled(),
    }),
  );
  const [defaultChatFirst, setDefaultChatFirst] = useState(() => chatFirst || isChatDefaultEnabled());
  useEffect(() => listenCurrentPaneUiMode(paneId, setMode), [paneId]);
  useEffect(() => {
    setDefaultChatFirst(chatFirst || isChatDefaultEnabled());
  }, [chatFirst]);
  useEffect(() => listenPaneUiModeDefaultChanged((enabled) => {
    setDefaultChatFirst(enabled);
    setMode(resolveInitialPaneUiMode(paneId, {
      chatCapable,
      chatViewFlag: pane?.chat_view === true,
      chatDefault: enabled,
    }));
  }), [paneId, chatCapable, pane?.chat_view]);

  if (!chatPty) return null;

  const baseOptions = defaultChatFirst ? [CHAT, TERMINAL] : [TERMINAL, CHAT];

  return (
    <Segmented
      data-testid="pane-mode-switch"
      data-pane-id={paneId}
      aria-label="Pane view"
      value={mode}
      options={baseOptions}
      height={height}
      onChange={(next) => {
        setMode(next);
        emitPaneUiMode(paneId, next);
      }}
    />
  );
}
