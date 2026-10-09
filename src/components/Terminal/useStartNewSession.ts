import { useRef, useState } from "react";
import { invoke } from "../../lib/bridge";

export async function startNewTabSession(tabId: string): Promise<void> {
  await invoke("restart_agent_tab", { tabId, fresh: true });
}

export function useStartNewSession(
  tabId: string,
  request: (tabId: string) => Promise<void> = startNewTabSession,
) {
  const inFlight = useRef(false);
  const [startingNewSession, setStarting] = useState(false);
  const [actionError, setError] = useState<string | null>(null);
  async function onStartNewSession() {
    if (inFlight.current) return;
    inFlight.current = true;
    setStarting(true);
    setError(null);
    try {
      await request(tabId);
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      inFlight.current = false;
      setStarting(false);
    }
  }
  return { onStartNewSession, startingNewSession, actionError };
}
