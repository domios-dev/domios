import { useMemo, useRef, type ReactElement } from "react";
import type { ChatSessionIntent, NormalizedChatMessage } from "@codeterm/chat-core";
import { sessionIntentFromMessages } from "@codeterm/chat-ui";
import type { AgentRuntimeState } from "../../lib/agentRuntimeState";
import { useLatestCallback } from "../../hooks/useLatestCallback";
import { AgentRuntimeBanner } from "./AgentRuntimeBanner";

export function useComposerPresentation({ tabId, nonce, state, retry, startupOwned, iconSvg, messages }: {
  tabId: string;
  nonce: string | null;
  state: AgentRuntimeState | null;
  retry: () => Promise<void>;
  startupOwned: boolean;
  iconSvg?: string | null;
  messages: readonly NormalizedChatMessage[];
}) {
  const onRetry = useLatestCallback(() => { void retry(); }, [tabId, nonce]);
  const bannerKey = JSON.stringify([tabId, nonce, state, startupOwned, iconSvg]);
  const previous = useRef<{ key: string; retry: typeof onRetry; node: ReactElement } | null>(null);
  if (previous.current?.key !== bannerKey || previous.current.retry !== onRetry) {
    previous.current = { key: bannerKey, retry: onRetry, node: <AgentRuntimeBanner
      tabId={tabId} state={state} startupOwned={startupOwned} iconSvg={iconSvg} onRetry={onRetry} /> };
  }
  const intent = useMemo(() => sessionIntentFromMessages(messages), [messages]);
  const title = intent?.title;
  const goal = intent?.goal;
  const goalStatus = intent?.goalStatus;
  const sessionIntent = useMemo<ChatSessionIntent | null>(() => title || goal
    ? { ...(title ? { title } : {}), ...(goal ? { goal } : {}), ...(goalStatus ? { goalStatus } : {}) }
    : null, [title, goal, goalStatus]);
  return { runtimeBadge: previous.current.node, sessionIntent };
}
