import { useRef, useState } from "react";
import { invoke } from "../../lib/bridge";

export function useRestartTab(tabId: string | undefined) {
  const inFlight = useRef(false);
  const [restarting, setRestarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function restart() {
    if (!tabId || inFlight.current) return;
    inFlight.current = true;
    setRestarting(true);
    setError(null);
    try {
      await invoke("restart_agent_tab", { tabId });
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      inFlight.current = false;
      setRestarting(false);
    }
  }
  return { restart, restarting, error };
}
