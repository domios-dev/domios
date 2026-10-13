import { RuntimeLoginActions } from "./RuntimeLoginActions";
import { runtimeBannerView, type AgentRuntimeState } from "../../lib/agentRuntimeState";
import { ChatStatusBanner } from "./ChatSetupState";
import { TabLoadingIndicator } from "../shared/TabLoadingIndicator";

export function AgentRuntimeBanner({ state, onRetry, tabId }: { tabId?:string; state: AgentRuntimeState | null; onRetry: () => void }) {
  if (tabId && state?.auth === "signedOut" && state.loginMethods?.length) return <ChatStatusBanner
    title="Sign-in required" reason={null} action={<div className="flex items-center gap-2"><RuntimeLoginActions key={`${tabId}:${state.epoch}`} tabId={tabId} state={state} /><button type="button" disabled={state.login?.status === "pending"} onClick={onRetry}>Check sign-in</button></div>} />;
  const view = runtimeBannerView(state);
  if (!view) return null;
  return <ChatStatusBanner title={view.loading ? "Recovering agent runtime" : "Agent runtime unavailable"}
    reason={view.reason} loading={view.loading}
    icon={view.loading ? <TabLoadingIndicator label="Loading agent runtime" compact deferred={false} role="img" /> : undefined}
    action={<button type="button" aria-label="Retry" onClick={onRetry} disabled={!view.canRetry}
      className="rounded-md border border-[var(--ct-border)] px-2.5 py-1 text-xs font-medium text-[var(--ct-fg)] transition-colors hover:bg-[var(--ct-border)] disabled:cursor-default disabled:opacity-50 disabled:hover:bg-transparent">Retry</button>} />;
}
