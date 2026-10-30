import { DsBlockScanner, type DsSegment } from "@codeterm/chat-core";
import { parsedMarkdown } from "./markdownParse";
import { normalizeMultilineHtmlBlocks } from "./htmlSanitize";

export interface MessagePart { id: string; start: number; end: number; renderedStart: number; renderedEnd: number }
interface Edit { start: number; end: number; text: string }
function originalOffset(offset: number, edits: readonly Edit[]): number {
  let delta = 0;
  for (const edit of edits) {
    const start = edit.start + delta;
    const finish = start + edit.text.length;
    if (offset < start) break;
    if (offset <= finish) return offset === finish ? edit.end : edit.start;
    delta += edit.text.length - (edit.end - edit.start);
  }
  return offset - delta;
}
export function normalizedMarkdown(source: string) {
  const edits: Edit[] = [];
  const text = normalizeMultilineHtmlBlocks(source, (start, end) => edits.push({ start, end, text: "\n" }));
  return { text, start: (offset: number) => originalOffset(offset, edits), end: (offset: number) => originalOffset(offset, edits) };
}
const MESSAGE_PARTS_CACHE_CHARS = 2_000_000;
const cache = new Map<string, { scanner: DsBlockScanner; parts: MessagePart[] }>();
let cachedChars = 0;
export function messageParts(id: string, source: string, baseOffset = 0): MessagePart[] {
  return cachedMessageParts(`source:${id}:${baseOffset}`, id, source, baseOffset);
}
export function markdownMessageParts(id: string, source: string, baseOffset: number): MessagePart[] {
  return cachedMessageParts(`markdown:${id}:${baseOffset}`, id, source, baseOffset);
}
function cachedMessageParts(key: string, id: string, source: string, baseOffset: number): MessagePart[] {
  let entry = cache.get(key);
  if (entry?.scanner.text === source) { cache.delete(key); cache.set(key, entry); return entry.parts; }
  if (entry) { cachedChars -= entry.scanner.text.length; cache.delete(key); }
  if (!entry || !source.startsWith(entry.scanner.text)) entry = { scanner: new DsBlockScanner({ id: key, trusted: true }), parts: [] };
  entry.scanner.append(source.slice(entry.scanner.text.length));
  entry.parts = parseParts(id, source, entry.scanner.segments, baseOffset);
  if (source.length <= MESSAGE_PARTS_CACHE_CHARS) {
    cache.set(key, entry); cachedChars += source.length;
    while (cache.size > 256 || cachedChars > MESSAGE_PARTS_CACHE_CHARS) {
      const oldest = cache.keys().next().value;
      if (oldest === undefined) break;
      cachedChars -= cache.get(oldest)!.scanner.text.length; cache.delete(oldest);
    }
  }
  return entry.parts;
}
function parseParts(id: string, source: string, segments: readonly DsSegment[], base: number): MessagePart[] {
  const ranges: Array<{ start: number; end: number; renderedStart: number; renderedEnd: number }> = [];
  for (const segment of segments) {
    if (segment.type !== "markdown") {
      ranges.push({ start: segment.start, end: segment.end, renderedStart: segment.start, renderedEnd: segment.end });
      continue;
    }
    const normalized = normalizedMarkdown(segment.text);
    const blocks = parsedMarkdown(normalized.text).children;
    const prose: typeof ranges = [];
    for (let index = 0; index < blocks.length; index++) {
      const block = blocks[index];
      if (block.type === "definition") continue;
      const start = block.position?.start.offset;
      let end = block.position?.end.offset;
      if (start === undefined || end === undefined) continue;
      if (block.type === "heading" && blocks[index + 1]?.type === "paragraph") end = blocks[++index].position?.end.offset ?? end;
      prose.push({ start: segment.start + normalized.start(start), end: segment.start + normalized.end(end), renderedStart: segment.start + start, renderedEnd: segment.start + end });
    }
    if (prose.length === 1 && segments.length === 1) {
      prose[0] = { start: 0, end: source.length, renderedStart: 0, renderedEnd: normalized.text.length };
    }
    for (const range of prose) ranges.push(range);
  }
  return ranges.map((range) => ({ ...range, id: `${id}:${base + range.start}`, start: base + range.start, end: base + range.end }));
}
