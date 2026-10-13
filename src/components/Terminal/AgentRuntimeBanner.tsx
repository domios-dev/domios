import { runtimeBannerView, type AgentRuntimeState } from "../../lib/agentRuntimeState";
import { ChatStatusBanner } from "./ChatSetupState";
import { TabLoadingIndicator } from "../shared/TabLoadingIndicator";

export function AgentRuntimeBanner({ state, onRetry }: { state: AgentRuntimeState | null; onRetry: () => void }) {
  const view = runtimeBannerView(state);
  if (!view) return null;
  return <ChatStatusBanner title={view.loading ? "Recovering agent runtime" : "Agent runtime unavailable"}
    reason={view.reason} loading={view.loading}
    icon={view.loading ? <TabLoadingIndicator label="Loading agent runtime" compact deferred={false} role="img" /> : undefined}
    action={<button type="button" aria-label="Retry" onClick={onRetry} disabled={!view.canRetry}
      className="rounded-md border px-2 py-1 text-xs font-medium disabled:opacity-50">Retry</button>} />;
}
