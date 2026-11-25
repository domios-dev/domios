import { useRef, useState } from "react";
import { runtimeStatusPresentation, type AgentRuntimeState } from "../../lib/agentRuntimeState";
import { describeRuntimeConnectionError, isMissingRuntimeConversation } from "../../lib/describeError";
import { parseProviderNotInstalled } from "../../lib/providerNotInstalled";
import { ProviderNotInstalledNotice } from "./ProviderNotInstalledNotice";
import { runtimeSignInRequired } from "./RuntimeLoginActions";
import { useStartNewSession } from "./useStartNewSession";
import { RuntimeTranscriptNotice } from "./TranscriptNotice";

export function AgentRuntimeBanner({ state, onRetry, tabId, startupOwned = false, centered = false }: {
  tabId?: string;
  state: AgentRuntimeState | null;
  onRetry: () => void | Promise<void>;
  startupOwned?: boolean;
  centered?: boolean;
}) {
  const fresh = useStartNewSession(tabId ?? "");
  const [retrying, setRetrying] = useState(false);
  const presentation = runtimeStatusPresentation(state, startupOwned);
  const actionable = presentation?.severity === "action_needed" && !runtimeSignInRequired(state) ? presentation : null;
  const previous = useRef(actionable);
  const view = retrying ? previous.current : actionable;
  if (!view) return null;
  const notInstalled = parseProviderNotInstalled(view.details ?? view.reason);
  if (notInstalled) {
    return <ProviderNotInstalledNotice
      info={notInstalled}
      rawReason={view.details ?? view.reason}
      inline
      onRetry={async () => {
        previous.current = view;
        setRetrying(true);
        try { await Promise.resolve(onRetry()); }
        finally { setRetrying(false); }
      }}
    />;
  }
  const missing = Boolean(tabId && isMissingRuntimeConversation(view.details));
  const sentence = missing ? "This conversation is no longer available."
    : view.label === "Agent runtime unavailable" ? describeRuntimeConnectionError(view.details).title : view.reason;

  return <RuntimeTranscriptNotice sentence={sentence} details={fresh.actionError ?? view.details ?? view.reason} centered={centered}
    action={missing ? { label: "Start a new conversation", loading: fresh.startingNewSession, run: () => { void fresh.onStartNewSession(); } }
      : { label: "Try again", loading: retrying, run: () => {
        previous.current = view;
        setRetrying(true);
        void Promise.resolve(onRetry()).finally(() => setRetrying(false));
      } }} />;
}
