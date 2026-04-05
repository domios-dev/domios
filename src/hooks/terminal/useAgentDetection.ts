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

        // Detect and save the agent session ID for future resume
        const tryDetectSession = () => {
          const currentPanes = stateRef.current.panes;
          const pane = currentPanes[paneId];
          if (!pane) return;
          if (pane.agent_session_id) return;

          const excludeIds = Object.values(currentPanes)
            .map((p) => p.agent_session_id)
            .filter((id): id is string => !!id);
          const paneCwd = pane.cwd;
          if (!paneCwd) {
            setTimeout(tryDetectSession, 2000);
            return;
          }
          invoke<string | null>("detect_claude_session_id", { cwd: paneCwd, exclude: excludeIds })
            .then((sessionId) => {
              if (sessionId) {
                invoke("mux_set_claude_session_id", { paneId, sessionId }).catch(() => {});
              } else {
                setTimeout(tryDetectSession, 3000);
              }
            })
            .catch(() => {
              setTimeout(tryDetectSession, 3000);
            });
        };
        tryDetectSession();
      }
    });

    return () => disposable.dispose();
  }, [termReady, termRef, paneId, setPaneTitle, setAgentType]);
}
