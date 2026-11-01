import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { DeliverySend } from "./chatDeliveryModel";

export function firstTurnLocalSends(sends: readonly DeliverySend[], ptyId: string | null | undefined, nonce: string | null | undefined): readonly DeliverySend[] {
  if (!ptyId || !nonce) return [];
  return sends.filter(send => !send.dismissed && send.localAttempt?.ptyId === ptyId
    && send.localAttempt.bindingNonce === nonce);
}

export function isFirstTurnLocalRow(row: NormalizedChatMessage, sends: readonly DeliverySend[]): boolean {
  return row.type === "user" && row.sourceSessionId == null && sends.some(send => row.id === send.id
    && row.correlationId === send.correlationId && row.content === send.content
    && row.timestamp === String(send.sentAtMs));
}
