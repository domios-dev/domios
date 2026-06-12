import type { NormalizedChatMessage } from "./types";

export type FingerprintableChatMessage = Pick<
  NormalizedChatMessage,
  "type" | "content" | "attachments"
>;

export function normalizeChatText(content: string): string {
  return content
    .replace(/\r\n/g, "\n")
    .replace(/\u0000codeterm-attached-file:[^\u0000]+\u0000/g, " ")
    .replace(/\[Image #[^\]]+\]/g, " ")
    .replace(/<image\b[^>]*>[\s\S]*?<\/image>/gi, " ")
    .replace(/^<local-command-stdout>[\s\S]*?<\/local-command-stdout>\s*/i, "")
    .replace(/\s+/g, " ")
    .trim();
}

export function chatMessageFingerprint(message: FingerprintableChatMessage): string | null {
  if (message.type !== "user") return null;
  const text = normalizeChatText(message.content);
  const attachmentKey = (message.attachments ?? [])
    .map((attachment) => attachment.id || attachment.path || attachment.url || attachment.mimeType || "")
    .filter(Boolean)
    .sort()
    .join("|");
  if (!text && !attachmentKey) return null;
  return `${text.toLowerCase()}::${attachmentKey}`;
}
