import { useEffect, useState } from "react";
import { Segmented, type SegmentedOption } from "../shared/Segmented";
import { emitPaneUiMode, isChatDefaultEnabled, type PaneUiMode } from "../../lib/chatUiMode";
import { listenCurrentPaneUiMode, resolveInitialPaneUiMode } from "../../lib/clientPaneUiMode";
import { useMultiplexer } from "../../store/multiplexer";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";

const TERMINAL: SegmentedOption<PaneUiMode> = { value: "terminal", label: "Terminal" };
const CHAT: SegmentedOption<PaneUiMode> = { value: "chat", label: "Chat" };

/** Terminal | Chat switch for a pane. Emitting the mode fully swaps the view —
 *  TerminalPane mounts/unmounts the chat surface in response. When chat is the
 *  default view, it leads the picker. The initial state mirrors TerminalPane's
 *  own resolution so the knob never starts on the wrong segment and slides. */
export function PaneModeSwitch({
  paneId,
  height,
  chatFirst = false,
}: {
  paneId: string;
  height?: number;
  chatFirst?: boolean;
}) {
  const { state } = useMultiplexer();
  const pane = state.panes[paneId];
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
  useEffect(() => listenCurrentPaneUiMode(paneId, setMode), [paneId]);

  return (
    <Segmented
      data-testid="pane-mode-switch"
      value={mode}
      options={chatFirst ? [CHAT, TERMINAL] : [TERMINAL, CHAT]}
      height={height}
      onChange={(next) => {
        setMode(next);
        emitPaneUiMode(paneId, next);
      }}
    />
  );
}
