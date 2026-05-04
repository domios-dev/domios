import { useEffect, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import { invoke } from "@tauri-apps/api/core";
import { useMultiplexer } from "../../store/multiplexer";
import { detectAgentFromTitle } from "../../lib/agentProviders";
import { TitleLevel } from "../../lib/types";

export interface UseAgentDetectionOptions {
  termRef: React.MutableRefObject<Terminal | null>;
  paneId: string;
  termReady: boolean;
}

export function useAgentDetection({
  termRef,
  paneId,
  termReady,
}: UseAgentDetectionOptions): void {
  const { state, setPaneTitle, setAgentType } = useMultiplexer();
  const stateRef = useRef(state);
  stateRef.current = state;

  // Path 1 — title-based detection. Works for claude (emits OSC title) but
  // not for codex/opencode (no OSC). Fires whenever xterm sees an OSC 0/1/2
  // sequence, even when the resulting title pattern doesn't match an agent.
  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady) return;

    const disposable = term.onTitleChange((title) => {
      if (!title) return;

      const curPane = stateRef.current.panes[paneId];
      if (curPane && curPane.title_level === TitleLevel.User) return;

      const hasAgent = curPane?.agent_type != null;
      setPaneTitle(paneId, title, hasAgent ? TitleLevel.Agent : TitleLevel.Auto);

      const detectedAgent = detectAgentFromTitle(title);
      const prevAgent = stateRef.current.panes[paneId]?.agent_type;

      // Agent exited: title no longer matches agent pattern
      if (!detectedAgent && prevAgent) {
        setAgentType(paneId, null);
        invoke("mux_set_pane_agent_type", { paneId, agentType: null }).catch(() => {});
      }

      if (detectedAgent) {
        setAgentType(paneId, detectedAgent);
        invoke("mux_set_pane_agent_type", { paneId, agentType: detectedAgent }).catch(() => {});
        // Reset session ID on first detection
        if (!prevAgent) {
          invoke("mux_set_claude_session_id", { paneId, sessionId: null }).catch(() => {});
        }
      }
      // Session detection fires from the agent_type-watcher effect below —
      // having one source of truth avoids racing the two paths.
    });

    return () => disposable.dispose();
  }, [termReady, termRef, paneId, setPaneTitle, setAgentType]);

  // Path 2 — used to be a frontend-side poll mirroring the backend's
  // `kick_off_session_detection`. Removed: the backend pinpoints detection
  // to the wall-clock instant the pane became an agent, while the frontend
  // had no such floor and would happily pick up sessions created seconds
  // before the spawn (race noticed when freshly-spawned opencode panes
  // hijacked sessions left over from rehydrated panes). The backend path is
  // single source of truth now; this hook only handles title-based detection.
}
