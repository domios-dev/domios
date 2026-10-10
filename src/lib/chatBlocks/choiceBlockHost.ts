import type { ChoiceBlockHost, ChoiceBlockState } from "@codeterm/chat-ui";
import type { ValueFeed } from "./valueFeed";

interface TranscriptMessage {
  id: string;
  type: string;
  content: string;
  inboundKind?: string;
}

/** Maps each assistant message to the user's next own reply in the transcript; inbound reports and tool rows are not replies. */
export function choiceRepliesByMessage(messages: readonly TranscriptMessage[]): ReadonlyMap<string, string> {
  const replies = new Map<string, string>();
  let waiting: string[] = [];
  for (const message of messages) {
    if (message.type === "assistant") {
      waiting.push(message.id);
    } else if (message.type === "user" && !message.inboundKind) {
      const reply = message.content.trim();
      for (const id of waiting) replies.set(id, reply);
      waiting = [];
    }
  }
  return replies;
}

/** A choice answers once per message: the transcript or an in-flight claim closes it, so a second tap never sends. */
export function createChoiceBlockHost(replies: ValueFeed<ReadonlyMap<string, string>>, send: (label: string) => Promise<void>): ChoiceBlockHost {
  const claimed = new Set<string>();
  let cachedFor = replies.get();
  const states = new Map<string, ChoiceBlockState>();
  return {
    binding: {
      subscribe: replies.subscribe,
      read(messageId) {
        const current = replies.get();
        if (current !== cachedFor) {
          cachedFor = current;
          states.clear();
        }
        let state = states.get(messageId);
        if (!state) {
          state = { reply: current.get(messageId) ?? null };
          states.set(messageId, state);
        }
        return state;
      },
      refresh: () => {},
    },
    async choose(messageId, label) {
      if (claimed.has(messageId) || replies.get().has(messageId)) return false;
      claimed.add(messageId);
      try {
        await send(label);
        return true;
      } catch (error) {
        claimed.delete(messageId);
        throw error;
      }
    },
  };
}
