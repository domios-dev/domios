import { useMuxSelector } from "../../store/multiplexer/hooks";
import { composerSendPolicy } from "./chatSetupModel";

export function useStartupSend(tabId: string) {
  return useMuxSelector(state => composerSendPolicy(state.tabs[tabId]),
    (a, b) => a.allowed === b.allowed && a.terminalFallback === b.terminalFallback
      && a.recipient?.provider_id === b.recipient?.provider_id
      && a.recipient?.binding_nonce === b.recipient?.binding_nonce
      && a.recipient?.session_id === b.recipient?.session_id);
}
