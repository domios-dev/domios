import { normalizeChatText } from "./normalize";
import type { NormalizedChatMessage } from "./types";

const SESSION_RESET_COMMANDS = new Set(["/clear", "/new"]);

function resetCommand(content: string): boolean {
  return SESSION_RESET_COMMANDS.has(normalizeChatText(content).toLowerCase());
}

export function lastSessionResetIndex<T extends Pick<NormalizedChatMessage, "type" | "content">>(
  messages: T[],
): number {
  for (let i = messages.length - 1; i >= 0; i--) {
    const message = messages[i];
    if (message.type === "user" && resetCommand(message.content)) {
      return i;
    }
  }
  return -1;
}

export function trimAfterSessionReset<T extends Pick<NormalizedChatMessage, "type" | "content">>(
  messages: T[],
): T[] {
  const index = lastSessionResetIndex(messages);
  return index >= 0 ? messages.slice(index) : messages;
}
