import type { NormalizedChatMessage } from "@codeterm/chat-core";

export function toolCallProgress(message: NormalizedChatMessage, now: number): string {
  const status = message.toolStatus;
  if (!status) return "";
  const deadline = message.toolInput?.deadlineAtMs;
  const stalled = status === "running" && message.toolKind === "command" &&
    typeof deadline === "number" && Number.isSafeInteger(deadline) && now >= deadline;
  const label = status === "abandoned" ? "Interrupted by a new turn" : stalled ? "Stalled" : status === "running" ? "Running" : status;
  if (status !== "running") return label;
  const start = message.toolStartedAtMs;
  const end = now;
  if (start == null) return label;
  const seconds = Math.floor(Math.max(0, end - start) / 1000);
  const duration = seconds >= 60 ? `${Math.floor(seconds / 60)}m ${seconds % 60}s` : `${seconds}s`;
  return `${label} · ${duration}`;
}
