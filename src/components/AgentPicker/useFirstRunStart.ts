import { useEffect, useRef, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import type { MultiplexerState, TabBootStageEvent } from "../../lib/types";
import { firstRunGeneralTarget } from "./firstRunNavigation";
import type { TabRef } from "./pickerNavigation";

export function useFirstRunStart(show: (target: TabRef) => Promise<void> | void) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const attempt = useRef<{ stop?: () => void } | null>(null);
  useEffect(() => () => { attempt.current?.stop?.(); attempt.current = null; }, []);

  const onConfirm = async (providerId: string) => {
    if (attempt.current) return;
    const current: { stop?: () => void } = {};
    attempt.current = current;
    setBusy(true);
    setError(null);
    let target: TabRef | null = null;
    const stages = new Map<string, TabBootStageEvent>();
    const settle = (event: TabBootStageEvent) => {
      if (attempt.current !== current || event.tab_id !== target?.tabId) return;
      if (event.stage === "agent_ready" || event.stage === "spawn_failed" || event.stage === "launch_deadline_elapsed" || event.stage === "launch_binding_unconfirmed") {
        setBusy(false);
        setError(event.stage === "agent_ready" ? null : event.reason || "Failed to start General agent");
        current.stop?.();
        attempt.current = null;
      }
    };
    try {
      current.stop = await listen<TabBootStageEvent>("tab-boot-stage", ({ payload }) => {
        stages.set(payload.tab_id, payload);
        settle(payload);
      });
      if (attempt.current !== current) { current.stop(); return; }
      await invoke("complete_general_agent_first_run", { providerId, fullAccess: true });
      const state = await invoke<MultiplexerState>("mux_get_state");
      if (attempt.current !== current) return;
      target = firstRunGeneralTarget(state);
      if (!target) throw new Error("General agent tab is unavailable");
      const tab = state.tabs[target.tabId];
      const event = stages.get(target.tabId);
      if (event) settle(event);
      if (tab.boot_stage) settle({ tab_id: tab.id, stage: tab.boot_stage, reason: tab.boot_failure_reason });
      await show(target);
    } catch (failure) {
      if (attempt.current !== current) return;
      setError(failure instanceof Error ? failure.message : typeof failure === "string" ? failure : "Failed to start General agent");
      setBusy(false);
      current.stop?.();
      attempt.current = null;
    }
  };
  return { busy, error, onConfirm };
}
