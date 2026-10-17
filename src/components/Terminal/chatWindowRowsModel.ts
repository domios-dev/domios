import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { isRenderableChatRow } from "@codeterm/chat-ui/renderability";

export interface ChatWindowRowSource {
  msg: NormalizedChatMessage;
}

export function hasChatWindowRow(message: NormalizedChatMessage): boolean {
  return isRenderableChatRow(message);
}

export function chatWindowRows<T extends ChatWindowRowSource>(rows: readonly T[]): T[] {
  return rows.filter(({ msg }) => hasChatWindowRow(msg));
}
