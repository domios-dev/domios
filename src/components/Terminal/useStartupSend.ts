import { useEffect, useRef, useState } from "react";
import { useMuxSelector } from "../../store/multiplexer";
import { chatSetupView } from "./chatSetupModel";
import { createStartupSendGate } from "./startupSendGate";

export function useStartupSend(tabId: string, chatBackend: boolean) {
  const nonce = useMuxSelector(state => state.tabs[tabId]?.session_binding_nonce ?? null);
  const starting = useMuxSelector(state => !chatBackend && !!state.tabs[tabId]
    && chatSetupView(state.tabs[tabId]).kind === "starting");
  const gate = useRef(createStartupSendGate(Date.now)).current;
  const [, refresh] = useState(0);
  gate.observe(`${tabId}:${nonce ?? ""}`, starting);
  const remaining = gate.remainingMs();
  useEffect(() => {
    if (remaining === 0) return;
    const timer = window.setTimeout(() => refresh(value => value + 1), remaining);
    return () => window.clearTimeout(timer);
  }, [tabId, nonce, starting, remaining]);
  return {
    allowed: remaining === 0,
    overrideNonce: starting && remaining === 0 ? nonce : null,
  };
}
