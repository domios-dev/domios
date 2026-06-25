import { parseAttachedDiscussionDoc } from "@codeterm/chat-ui";

function docBasename(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}

// Strip markdown to a flat one-paragraph digest for Card mode (no full render):
// drop code fences, headings, list/quote markers, emphasis, links → first prose.
export function digestMarkdown(text: string, maxChars = 160): string {
  const attached = parseAttachedDiscussionDoc(text);
  const source = attached
    ? (attached.summary || `doc: ${docBasename(attached.path)}`)
    : text;
  const cleaned = source
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^[\s>#*\-+]+/gm, "")
    .replace(/[*_~]/g, "")
    .replace(/\s+/g, " ")
    .trim();
  if (cleaned.length <= maxChars) return cleaned;
  return `${cleaned.slice(0, maxChars).trimEnd()}…`;
}
