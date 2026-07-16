import { mergeTimelineMessages } from "./timeline";
import type { NormalizedChatMessage } from "./types";

/**
 * A parser sequence is only unique inside its transcript session. Optimistic
 * entries receive the next parser sequence at insertion, so a persisted copy
 * replaces that entry without inspecting its content.
 */
export function transcriptMessageIdentity(
  message: Pick<NormalizedChatMessage, "id" | "seq" | "transcriptIdentity">,
  sessionId: string | null,
): string {
  if (message.transcriptIdentity) return message.transcriptIdentity;
  if (message.seq != null) return `session:${sessionId ?? "unbound"}:seq:${message.seq}`;
  return `message:${message.id}`;
}

export function projectTranscriptMessages<T extends NormalizedChatMessage>(
  confirmed: T[],
  optimistic: T[],
  sessionId: string | null,
): T[] {
  if (optimistic.length === 0) return confirmed;
  const confirmedIdentities = new Set(
    confirmed.map((message) => transcriptMessageIdentity(message, sessionId)),
  );
  return mergeTimelineMessages(
    confirmed,
    optimistic.filter((message) => !confirmedIdentities.has(transcriptMessageIdentity(message, sessionId))),
  );
}
