import { markdownToPlainText } from "../../../lib/markdownText";

export function reportActivityBody(text: string): string {
  return text.replace(/^(?:In Review|Done|Blocked|Partial):\s*/i, "").trim();
}

export function reportActivitySummary(text: string): string {
  const body = reportActivityBody(text).replace(/^(?:#{1,6}\s+[^\n]*\n\s*)+/, "");
  const plain = markdownToPlainText(body).replace(/\s+/g, " ").trim();
  const sentenceEnd = /[.!?](?:["')\]]*)\s/;
  const match = sentenceEnd.exec(plain);
  return match ? plain.slice(0, match.index + match[0].trimEnd().length) : plain;
}
