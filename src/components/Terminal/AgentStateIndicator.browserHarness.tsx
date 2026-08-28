import { createRoot, type Root } from "react-dom/client";
import { AgentStateIndicator, type AgentState } from "./AgentStateIndicator";
import { isCodeTermFrontendTestMode } from "./chatTestBridge";

declare global {
  interface Window {
    __codetermAgentStateHarness?: {
      mountQuiet: (host: HTMLElement) => Root;
    };
  }
}

export function mountQuietAgentStateHarness(host: HTMLElement): Root {
  const root = createRoot(host);
  root.render(
    <AgentStateIndicator
      state={{ state: "quiet" as AgentState["state"] }}
      providerName="Codex"
      model="gpt-5.6-sol"
    />,
  );
  return root;
}

export function installAgentStateIndicatorHarness(testMode = isCodeTermFrontendTestMode()): void {
  if (!testMode || typeof window === "undefined" || window.__codetermAgentStateHarness) return;
  window.__codetermAgentStateHarness = { mountQuiet: mountQuietAgentStateHarness };
}
