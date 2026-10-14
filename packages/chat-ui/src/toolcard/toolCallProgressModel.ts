import type { NormalizedChatMessage } from "@codeterm/chat-core";

export function toolCallProgress(message: NormalizedChatMessage, now: number): string {
  const status = message.toolStatus;
  if (!status) return "";
  const label = status === "abandoned" ? "Interrupted by a new turn" : status === "running" ? "Running" : status;
  const start = message.toolStartedAtMs;
  const end = status === "running" ? now : message.toolCompletedAtMs;
  if (start == null || end == null) return label;
  const seconds = Math.floor(Math.max(0, end - start) / 1000);
  const duration = seconds >= 60 ? `${Math.floor(seconds / 60)}m ${seconds % 60}s` : `${seconds}s`;
  return `${label} · ${duration}`;
}
