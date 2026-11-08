import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { DeliverySend } from "./chatDeliveryModel";

export function firstTurnLocalSends(sends: readonly DeliverySend[], ptyId: string | null | undefined, nonce: string | null | undefined): readonly DeliverySend[] {
  if (!ptyId || !nonce) return [];
  return sends.filter(send => !send.dismissed && send.localAttempt?.ptyId === ptyId
    && send.localAttempt.bindingNonce === nonce);
}

/** A first-turn send of a replaced attempt waits for transcript authority; every other send belongs to this launch. */
export function sendsOfCurrentAttempt(sends: readonly DeliverySend[], ptyId: string | null | undefined, nonce: string | null | undefined): readonly DeliverySend[] {
  return sends.filter(send => !send.localAttempt || (send.localAttempt.ptyId === ptyId && send.localAttempt.bindingNonce === nonce));
}

/** The row is exactly one of these sends' own local bubbles, not a transcript row with similar content. */
export function isLocalSendRow(row: NormalizedChatMessage, sends: readonly DeliverySend[]): boolean {
  return row.type === "user" && row.sourceSessionId == null && sends.some(send => row.id === send.id
    && row.correlationId === send.correlationId && row.content === send.content
    && row.timestamp === String(send.sentAtMs));
}
