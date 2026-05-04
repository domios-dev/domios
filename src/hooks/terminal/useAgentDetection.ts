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

/// Polls `detect_pane_session_id` until it returns an id or we exhaust 60s.
/// Lives outside the title-change closure so it can also fire from the
/// agent_type-changed effect (codex/opencode panes don't emit OSC titles —
/// without this fallback they never get a saved session_id and silently
/// fail to resume on app restart).
function pollSessionId(
  paneId: string,
  providerId: string,
  getPanes: () => Record<string, { agent_session_id?: string | null; cwd?: string | null } | undefined>,
  attempt = 0,
) {
  const MAX_ATTEMPTS = 60; // 60 × 3s ≈ 180s budget — slow providers (github-copilot, auth flows) can take >1 min
  if (attempt > MAX_ATTEMPTS) return;
  const panes = getPanes();
  const pane = panes[paneId];
  if (!pane) return;
  if (pane.agent_session_id) return;
  const excludeIds = Object.values(panes)
    .map((p) => p?.agent_session_id)
    .filter((id): id is string => !!id);
  const paneCwd = pane.cwd;
  if (!paneCwd) {
    setTimeout(() => pollSessionId(paneId, providerId, getPanes, attempt + 1), 2000);
    return;
  }
  // Cap detection window to 5 min (matches backend `kick_off_session_detection`).
  // Without a cutoff, any stale session in the same cwd from a prior pane
  // gets pinned to a fresh one — see opencode/codex multi-pane spawn race.
  invoke<string | null>("detect_pane_session_id", {
    providerId,
    cwd: paneCwd,
    exclude: excludeIds,
    maxAgeSecs: 300,
  })
    .then((sessionId) => {
      if (sessionId) {
        invoke("mux_set_claude_session_id", { paneId, sessionId }).catch(() => {});
      } else {
        setTimeout(() => pollSessionId(paneId, providerId, getPanes, attempt + 1), 3000);
      }
    })
    .catch(() => {
      setTimeout(() => pollSessionId(paneId, providerId, getPanes, attempt + 1), 3000);
    });
}

export function useAgentDetection({
  termRef,
  paneId,
  termReady,
}: UseAgentDetectionOptions): void {
  const { state, setPaneTitle, setAgentType } = useMultiplexer();
  const stateRef = useRef(state);
  stateRef.current = state;
  const sessionPollStartedFor = useRef<string | null>(null);

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

  // Path 2 — agent_type-based session polling. When the backend output
  // detector (or any other path) sets agent_type on a pane, we kick off
  // session-id polling regardless of how detection happened. Codex/OpenCode
  // depend on this entirely; Claude double-fires and the second call is a
  // no-op once agent_session_id is filled.
  const currentAgent = state.panes[paneId]?.agent_type ?? null;
  const currentSession = state.panes[paneId]?.agent_session_id ?? null;
  useEffect(() => {
    if (!currentAgent) {
      sessionPollStartedFor.current = null;
      return;
    }
    if (currentSession) {
      sessionPollStartedFor.current = currentAgent;
      return;
    }
    if (sessionPollStartedFor.current === currentAgent) return;
    sessionPollStartedFor.current = currentAgent;
    pollSessionId(paneId, currentAgent, () => stateRef.current.panes);
  }, [currentAgent, currentSession, paneId]);
}
