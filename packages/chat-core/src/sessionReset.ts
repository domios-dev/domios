import { normalizeChatText } from "./normalize";
import type { NormalizedChatMessage } from "./types";

const SESSION_RESET_COMMANDS = new Set(["/clear", "/new"]);
// Local command messages may contain bare tag content such as "new".
const SESSION_RESET_BARE = new Set(["clear", "new"]);

function resetCommand(content: string): boolean {
  return SESSION_RESET_COMMANDS.has(normalizeChatText(content).toLowerCase());
}

function isLocalCommandReset(content: string): boolean {
  const bare = normalizeChatText(content).toLowerCase().replace(/^\//, "");
  return SESSION_RESET_BARE.has(bare);
}

export function isSessionResetMessage<T extends Pick<NormalizedChatMessage, "type" | "content">>(
  message: T,
): boolean {
  return (message.type === "user" && resetCommand(message.content))
    || (message.type === "local_command" && isLocalCommandReset(message.content));
}

export function lastSessionResetIndex<T extends Pick<NormalizedChatMessage, "type" | "content">>(
  messages: T[],
): number {
  for (let i = messages.length - 1; i >= 0; i--) {
    const message = messages[i];
    if (isSessionResetMessage(message)) return i;
  }
  return -1;
}

export function trimAfterSessionReset<T extends Pick<NormalizedChatMessage, "type" | "content">>(
  messages: T[],
): T[] {
  const index = lastSessionResetIndex(messages);
  return index >= 0 ? messages.slice(index) : messages;
}
