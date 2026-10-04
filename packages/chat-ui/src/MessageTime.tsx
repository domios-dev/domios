import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { timestampMs } from "@codeterm/chat-core";

export type MessageTimeSide = "left" | "right";

export function formatMessageTime(timestamp: number, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit" }).format(timestamp);
}

export function MessageTime({ timestamp, side }: { timestamp?: NormalizedChatMessage["timestamp"]; side: MessageTimeSide }) {
  const timeMs = timestampMs(timestamp);
  if (timeMs == null) return null;

  const date = new Date(timeMs);
  if (!Number.isFinite(date.getTime())) return null;

  return (
    <time
      className={`ct-chat-message-time ct-chat-message-time--${side} ct-chat-message-time--hidden-when-narrow`}
      data-testid="chat-message-time"
      dateTime={date.toISOString()}
      title={new Intl.DateTimeFormat(undefined, { dateStyle: "full", timeStyle: "short" }).format(date)}
    >
      {formatMessageTime(timeMs)}
    </time>
  );
}
