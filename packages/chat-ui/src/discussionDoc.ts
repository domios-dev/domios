export interface AttachedDiscussionDoc {
  summary: string;
  path: string;
  body: string;
}

const DOC_TAIL_RE = /(?:^|\n\n)doc:\s*([^\n]+)\n\n([\s\S]*)$/;

export function parseAttachedDiscussionDoc(text: string): AttachedDiscussionDoc | null {
  const m = text.match(DOC_TAIL_RE);
  if (!m) return null;
  const path = m[1].trim();
  const body = m[2];
  if (!path || !body.trim()) return null;
  const headEnd = text.length - m[0].length;
  const summary = text.slice(0, headEnd).trim();
  return { summary, path, body };
}
