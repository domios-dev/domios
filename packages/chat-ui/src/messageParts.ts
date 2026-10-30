import { parsedMarkdown } from "./markdownParse";
import { isolateBlockDirectives } from "./blockDirectives";
import { normalizeMultilineHtmlBlocks } from "./htmlSanitize";

export interface MessagePart {
  id: string;
  start: number;
  end: number;
  renderedStart: number;
  renderedEnd: number;
}

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
  const insertions: Edit[] = [];
  const isolated = isolateBlockDirectives(source, (offset) => insertions.push({ start: offset, end: offset, text: "\n" }));
  const replacements: Edit[] = [];
  const text = normalizeMultilineHtmlBlocks(isolated, (start, end) => replacements.push({ start, end, text: "\n" }));
  const original = (offset: number) => originalOffset(originalOffset(offset, replacements), insertions);
  return { text, start: (offset: number) => original(offset), end: (offset: number) => original(offset) };
}

const MESSAGE_PARTS_CACHE_CHARS = 2_000_000;
const cache = new Map<string, { source: string; parts: MessagePart[] }>();
let cachedChars = 0;

export function messageParts(id: string, source: string): MessagePart[] {
  const previous = cache.get(id);
  if (previous?.source === source) {
    cache.delete(id);
    cache.set(id, previous);
    return previous.parts;
  }
  if (previous) {
    cachedChars -= previous.source.length;
    cache.delete(id);
  }
  const parts = parseMessageParts(id, source);
  // Entries hold only a message's own source and its offsets, so long messages stay cached instead of re-parsing per render.
  if (source.length <= MESSAGE_PARTS_CACHE_CHARS) {
    cache.set(id, { source, parts });
    cachedChars += source.length;
    while (cache.size > 256 || cachedChars > MESSAGE_PARTS_CACHE_CHARS) {
      const oldest = cache.keys().next().value;
      if (oldest === undefined) break;
      cachedChars -= cache.get(oldest)!.source.length;
      cache.delete(oldest);
    }
  }
  return parts;
}

function parseMessageParts(id: string, source: string): MessagePart[] {
  const normalized = normalizedMarkdown(source);
  const blocks = parsedMarkdown(normalized.text).children;
  const ranges: Array<{ start: number; end: number }> = [];
  for (let index = 0; index < blocks.length; index++) {
    const block = blocks[index];
    if (block.type === "definition") continue;
    const start = block.position?.start.offset;
    let end = block.position?.end.offset;
    if (start === undefined || end === undefined) continue;
    const next = blocks[index + 1];
    if (block.type === "heading" && next?.type === "paragraph" && !/^@ds-/.test(normalized.text.slice(next.position?.start.offset))) {
      end = next.position?.end.offset ?? end;
      index++;
    }
    ranges.push({ start, end });
  }
  if (ranges.length <= 1) return [{ id, start: 0, end: source.length, renderedStart: 0, renderedEnd: normalized.text.length }];
  return ranges.map(({ start, end }) => {
    const originalStart = normalized.start(start);
    const originalEnd = normalized.end(end);
    return { id: `${id}:${originalStart}-${originalEnd}`, start: originalStart, end: originalEnd, renderedStart: start, renderedEnd: end };
  });
}
