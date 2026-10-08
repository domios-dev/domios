import { useLayoutEffect, useMemo, useRef } from "react";
import type { ChoiceBlockHost } from "@codeterm/chat-ui";
import { choiceRepliesByMessage, createChoiceBlockHost } from "../../lib/chatBlocks/choiceBlockHost";
import { createValueFeed } from "../../lib/chatBlocks/valueFeed";
import { sendTrackedChatMessage, type ChatSendStatus } from "./chatSendStatus";

interface ChoiceSendPath {
  messages: readonly { id: string; type: string; content: string; inboundKind?: string }[];
  addOptimistic(content: string): string;
  followSentMessage(): void;
  observeChatSendStatus(status: ChatSendStatus): void;
}

/** This tab's choice service: a tap goes through the composer's own tracked send, as the user's reply. */
export function useTabChoiceHost(tabId: string, path: ChoiceSendPath): ChoiceBlockHost {
  const replies = useMemo(() => createValueFeed<ReadonlyMap<string, string>>(new Map()), [tabId]);
  const pathRef = useRef(path);
  pathRef.current = path;
  const computed = useMemo(() => choiceRepliesByMessage(path.messages), [path.messages]);
  useLayoutEffect(() => replies.set(computed), [replies, computed]);
  return useMemo(() => createChoiceBlockHost(replies, async (label) => {
    const { addOptimistic, followSentMessage, observeChatSendStatus } = pathRef.current;
    followSentMessage();
    const messageId = addOptimistic(label);
    observeChatSendStatus(await sendTrackedChatMessage(tabId, messageId, label));
  }), [replies, tabId]);
}
