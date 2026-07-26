import { useEffect, useState, useCallback } from "react";
import { Segmented, type SegmentedOption } from "../shared/Segmented";
import { emitPaneUiMode, type PaneUiMode } from "../../lib/chatUiMode";
import {
  listenCurrentPaneUiMode,
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
  chatPty = true,
}: {
  paneId: string;
  height?: number;
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
    }),
  );
  useEffect(() => listenCurrentPaneUiMode(paneId, setMode), [paneId]);

  if (!chatPty) return null;

  return (
    <Segmented
      data-testid="pane-mode-switch"
      data-pane-id={paneId}
      aria-label="Tab view"
      value={mode}
      options={[CHAT, TERMINAL]}
      height={height}
      onChange={(next) => {
        setMode(next);
        emitPaneUiMode(paneId, next);
      }}
    />
  );
}
