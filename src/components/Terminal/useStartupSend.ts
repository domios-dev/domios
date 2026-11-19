import { useMuxSelector } from "../../store/multiplexer/hooks";
import { composerSendPolicy } from "./chatSetupModel";

export function useStartupSend(tabId: string) {
  return useMuxSelector(state => composerSendPolicy(state.tabs[tabId]),
    (a, b) => a.allowed === b.allowed && a.terminalFallback === b.terminalFallback);
}
