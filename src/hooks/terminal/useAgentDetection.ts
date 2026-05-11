import { useEffect, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import { useMultiplexer } from "../../store/multiplexer";
import { TitleLevel } from "../../lib/types";

export interface UseAgentDetectionOptions {
  termRef: React.MutableRefObject<Terminal | null>;
  paneId: string;
  termReady: boolean;
}

// Identity (`agent_type`) is owned by the backend: spawn paths lock it, and
// `commands::pty::write_pty` parses typed launch commands. This hook only
// renders OSC titles into the pane title — it never sets agent_type.
export function useAgentDetection({
  termRef,
  paneId,
  termReady,
}: UseAgentDetectionOptions): void {
  const { state, setPaneTitle } = useMultiplexer();
  const stateRef = useRef(state);
  stateRef.current = state;

  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady) return;

    const disposable = term.onTitleChange((title) => {
      if (!title) return;
      const curPane = stateRef.current.panes[paneId];
      if (curPane && curPane.title_level === TitleLevel.User) return;
      const hasAgent = curPane?.agent_type != null;
      setPaneTitle(paneId, title, hasAgent ? TitleLevel.Agent : TitleLevel.Auto);
    });

    return () => disposable.dispose();
  }, [termReady, termRef, paneId, setPaneTitle]);
}
