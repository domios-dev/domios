import { isSessionResetMessage } from "./sessionReset";
import type { NormalizedChatMessage } from "./types";

export const CONTEXT_EPOCH_BOUNDARY_TEXT =
  "Context reset. Earlier turns are archived here and are not sent to the provider.";

export function projectContextEpochs<T extends NormalizedChatMessage>(
  messages: T[],
): NormalizedChatMessage[] {
  let epoch = 0;
  const projected: NormalizedChatMessage[] = [];
  let awaitingPersistedBoundary = false;
  for (let index = 0; index < messages.length; index += 1) {
    const message = messages[index];
    if (isSessionResetMessage(message)) {
      epoch += 1;
      projected.push(message.contextEpoch === epoch ? message : { ...message, contextEpoch: epoch });
      awaitingPersistedBoundary = false;
      for (let candidate = index + 1; candidate < messages.length; candidate += 1) {
        const candidateMessage = messages[candidate];
        if (!candidateMessage) continue;
        if (candidateMessage.systemSubtype === "context_epoch_boundary") {
          awaitingPersistedBoundary = true;
          break;
        }
        if (isSessionResetMessage(candidateMessage)) break;
      }
      if (!awaitingPersistedBoundary) {
        projected.push({
          id: `${message.id}:context-epoch-${epoch}`,
          type: "system",
          content: CONTEXT_EPOCH_BOUNDARY_TEXT,
          systemSubtype: "context_epoch_boundary",
          contextEpoch: epoch,
          timestamp: message.timestamp,
          seq: message.seq == null ? undefined : message.seq + 0.5,
        });
      }
      continue;
    }
    if (message.systemSubtype === "context_epoch_boundary") {
      if (!awaitingPersistedBoundary) epoch += 1;
      awaitingPersistedBoundary = false;
    }
    projected.push(message.contextEpoch === epoch ? message : { ...message, contextEpoch: epoch });
  }
  return projected;
}
