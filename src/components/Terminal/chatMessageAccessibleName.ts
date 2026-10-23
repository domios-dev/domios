import { attachmentTagsAsLabels } from "@codeterm/chat-core";

// Runs for every row on every streamed chunk; only the head of a long message can reach the excerpt.
const ACCESSIBLE_NAME_SCAN_CHARS = 1_000;

export function chatMessageAccessibleName(type: string, content: string): string {
  const head = content.length > ACCESSIBLE_NAME_SCAN_CHARS ? content.slice(0, ACCESSIBLE_NAME_SCAN_CHARS) : content;
  let compact = attachmentTagsAsLabels(head).replace(/\s+/g, " ").trim();
  if (compact.length <= 80 && head.length < content.length) compact = attachmentTagsAsLabels(content).replace(/\s+/g, " ").trim();
  const excerpt = compact.length > 80 ? `${compact.slice(0, 77)}…` : compact;
  return excerpt ? `${type} message: ${excerpt}` : `${type} message`;
}
