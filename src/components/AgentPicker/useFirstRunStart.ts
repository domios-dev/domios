import { useEffect, useReducer, useRef, useState } from "react";
import { recordProductEvent } from "../../lib/productAnalytics";
import { invoke, listen } from "../../lib/bridge";
import type { MultiplexerState, TabBootStageEvent } from "../../lib/types";
import { firstRunGeneralTarget } from "./firstRunNavigation";
import { installAndWait } from "../../lib/firstRunInstall";
import { firstRunTransition } from "../../lib/firstRunFlow";
import type { TabRef } from "./pickerNavigation";

export function useFirstRunStart(show: (target: TabRef) => Promise<void> | void, beforeStart?: (providerId: string, signal: AbortSignal) => Promise<void>) {
  const [phase, transition] = useReducer(firstRunTransition, "choosing");
  const busy = phase === "installing" || phase === "signIn" || phase === "starting";
  const [progress, setProgress] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const attempt = useRef<{ stop?: () => void; deadline?: ReturnType<typeof setTimeout>; controller: AbortController } | null>(null);
  useEffect(() => () => { attempt.current?.controller.abort(); attempt.current?.stop?.(); clearTimeout(attempt.current?.deadline); attempt.current = null; }, []);

  const onConfirm = async (providerId: string, install = false) => {
    if (attempt.current) return;
    const current: NonNullable<typeof attempt.current> = { controller: new AbortController() };
    attempt.current = current;
    transition(install ? "install" : "start");
    setError(null);
    setProgress(install ? "Installing harness…" : "Starting General agent…");
    let target: TabRef | null = null;
    const stages = new Map<string, TabBootStageEvent>();
    const fail = (reason: string) => {
      if (attempt.current !== current) return;
      setError(reason);
      transition("failure");
      setProgress(null);
      current.stop?.();
      clearTimeout(current.deadline);
      attempt.current = null;
    };
    const settle = (event: TabBootStageEvent) => {
      if (attempt.current !== current || event.tab_id !== target?.tabId) return;
      if (event.stage === "agent_ready" || event.stage === "spawn_failed") {
        setProgress(event.stage === "agent_ready" ? "Ready" : null);
        transition(event.stage === "agent_ready" ? "ready" : "failure");
        setError(event.stage === "agent_ready" ? null : event.reason || "Failed to start General agent");
        if (event.stage === "agent_ready") void recordProductEvent({ event_type: "onboarding_completed", event_properties: {} });
        current.stop?.();
        clearTimeout(current.deadline);
        attempt.current = null;
      }
    };
    try {
      current.stop = await listen<TabBootStageEvent>("tab-boot-stage", ({ payload }) => {
        stages.set(payload.tab_id, payload);
        settle(payload);
      });
      if (attempt.current !== current) { current.stop(); return; }
      if (install) {
        await installAndWait(providerId, current.controller.signal);
        if (attempt.current !== current) return;
        transition("installed");
        setProgress("Installation complete. Checking sign-in…");
      }
      await beforeStart?.(providerId, current.controller.signal);
      if (attempt.current !== current) return;
      transition("authenticated");
      setProgress("Starting General agent…");
      current.deadline = setTimeout(() => fail("Timed out waiting for the General agent to become ready."), 300_000);
      await invoke("complete_general_agent_first_run", { providerId });
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
      fail(failure instanceof Error ? failure.message : typeof failure === "string" ? failure : "Failed to start General agent");
    }
  };
  return { busy, phase, error, progress, onConfirm, onInstall: (providerId: string) => { void onConfirm(providerId, true); } };
}
