import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { timestampMs } from "@codeterm/chat-core";

export type MessageTimeSide = "left" | "right";

// Constructing an Intl.DateTimeFormat costs far more than formatting; a long transcript formats every row.
const shortTimeFormats = new Map<string, Intl.DateTimeFormat>();
let fullTimeFormat: Intl.DateTimeFormat | null = null;

export function formatMessageTime(timestamp: number, locale?: string): string {
  const key = locale ?? "";
  let format = shortTimeFormats.get(key);
  if (!format) {
    format = new Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit" });
    shortTimeFormats.set(key, format);
  }
  return format.format(timestamp);
}

function formatFullMessageTime(date: Date): string {
  fullTimeFormat ??= new Intl.DateTimeFormat(undefined, { dateStyle: "full", timeStyle: "short" });
  return fullTimeFormat.format(date);
}

export function MessageTime({ timestamp, side }: { timestamp?: NormalizedChatMessage["timestamp"]; side: MessageTimeSide }) {
  const timeMs = timestampMs(timestamp);
  if (timeMs == null) return null;

  const date = new Date(timeMs);
  if (!Number.isFinite(date.getTime())) return null;

  return (
    <time
      className={`ct-chat-message-time ct-chat-message-time--${side}`}
      data-testid="chat-message-time"
      dateTime={date.toISOString()}
      title={formatFullMessageTime(date)}
      aria-label={formatFullMessageTime(date)}
    >
      {formatMessageTime(timeMs)}
    </time>
  );
}
