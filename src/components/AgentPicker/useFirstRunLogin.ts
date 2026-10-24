import { useEffect, useRef, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import type { AgentRuntimeState, AgentRuntimeStateEvent } from "../../lib/agentRuntimeState";
import type { ProviderDescriptor } from "../../lib/types";
import type { TabRef } from "./pickerNavigation";

export function useFirstRunLogin(home: TabRef | undefined, providers: ProviderDescriptor[]) {
  const [login, setLogin] = useState<{ providerId: string; state: AgentRuntimeState } | null>(null);
  const stop = useRef<(() => void) | undefined>(undefined);
  useEffect(() => () => stop.current?.(), []);

  async function beforeStart(providerId: string, signal: AbortSignal) {
    const provider = providers.find(provider => provider.id === providerId);
    if (!provider?.cli_login || !home) return;
    let finish: () => void = () => {};
    const authenticated = new Promise<void>((resolve) => { finish = resolve; });
    const abort = () => finish();
    let latest: AgentRuntimeState | undefined;
    const observe = (state: AgentRuntimeState) => {
      latest = state;
      setLogin({ providerId, state });
      if (state.auth === "signedIn") finish();
    };
    stop.current = await listen<AgentRuntimeStateEvent>("agent-runtime-state", ({ payload }) => {
      if (payload.tab_id === home.tabId && payload.session_binding_nonce === null) observe(payload.state);
    });
    signal.addEventListener("abort", abort, { once: true });
    try {
      const result = await invoke<{ state: AgentRuntimeState }>("agent_runtime_login", {
        tabId: home.tabId, request: { action: "check", providerId },
      });
      if (!latest) observe(result.state);
      if (signal.aborted) throw new Error("Sign-in canceled");
      await authenticated;
      if (signal.aborted) throw new Error("Sign-in canceled");
      setLogin(null);
    } finally {
      stop.current?.();
      stop.current = undefined;
      signal.removeEventListener("abort", abort);
    }
  }
  return { login, beforeStart };
}
