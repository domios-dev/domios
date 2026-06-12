import { chatMessageFingerprint, normalizeChatText } from "./normalize";
import type { NormalizedChatMessage } from "./types";

export function dedupeRecentUserMessages<T extends NormalizedChatMessage>(
  messages: T[],
  windowSize = 30,
): T[] {
  const out: T[] = [];
  const recentKeys: string[] = [];
  for (const message of messages) {
    const key = chatMessageFingerprint(message);
    if (key && recentKeys.includes(key)) continue;
    out.push(message);
    if (key) {
      recentKeys.push(key);
      if (recentKeys.length > windowSize) recentKeys.shift();
    }
  }
  return out;
}

export function matchesRecentUserMessage<T extends NormalizedChatMessage>(
  message: T,
  recentMessages: T[],
): boolean {
  const key = chatMessageFingerprint(message);
  const text = normalizeChatText(message.content).toLowerCase();
  if (!key && !text) return false;
  return recentMessages.some((candidate) => {
    if (candidate.type !== "user") return false;
    if (key && chatMessageFingerprint(candidate) === key) return true;
    const candidateText = normalizeChatText(candidate.content).toLowerCase();
    return Boolean(text && candidateText.includes(text));
  });
}
