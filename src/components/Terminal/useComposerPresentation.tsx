import { useMemo } from "react";
import type { ChatSessionIntent, NormalizedChatMessage } from "@codeterm/chat-core";
import { sessionIntentFromMessages } from "@codeterm/chat-ui";

export function useSessionIntent(messages: readonly NormalizedChatMessage[]) {
  const intent = useMemo(() => sessionIntentFromMessages(messages), [messages]);
  const title = intent?.title;
  const goal = intent?.goal;
  const goalStatus = intent?.goalStatus;
  return useMemo<ChatSessionIntent | null>(() => title || goal
    ? { ...(title ? { title } : {}), ...(goal ? { goal } : {}), ...(goalStatus ? { goalStatus } : {}) }
    : null, [title, goal, goalStatus]);
}
