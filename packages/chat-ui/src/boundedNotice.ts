export const NOTICE_SUMMARY_MAX_CHARS = 160;

export interface BoundedNotice {
  summary: string;
  /** The full raw text, present only when the summary leaves part of it out. */
  details: string | null;
}

/** One bounded line for a status or error row: the first sentence of the first line that has words. */
export function boundedNotice(text: string): BoundedNotice {
  const raw = text.trim();
  const line = raw.split(/\r?\n/).map((part) => part.trim()).find((part) => /\p{L}/u.test(part)) ?? raw;
  const sentence = line.match(/^.*?[.!?](?=\s|$)/u)?.[0] ?? line;
  const chars = Array.from(sentence);
  const summary = chars.length > NOTICE_SUMMARY_MAX_CHARS
    ? `${chars.slice(0, NOTICE_SUMMARY_MAX_CHARS - 1).join("").trimEnd()}…`
    : sentence;
  return { summary, details: summary === raw ? null : raw };
}
