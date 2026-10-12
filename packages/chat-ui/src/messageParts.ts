import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
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

const parser = unified().use(remarkParse).use(remarkGfm);
const cache = new Map<string, { source: string; parts: MessagePart[] }>();
let cachedChars = 0;

export function messageParts(id: string, source: string): MessagePart[] {
  const previous = cache.get(id);
  if (previous?.source === source) return previous.parts;
  if (previous) {
    cachedChars -= previous.source.length;
    cache.delete(id);
  }
  const parts = parseMessageParts(id, source);
  if (source.length <= 50_000) {
    cache.set(id, { source, parts });
    cachedChars += source.length;
    while (cache.size > 256 || cachedChars > 500_000) {
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
  const blocks = parser.parse(normalized.text).children;
  const ranges: Array<{ start: number; end: number }> = [];
  for (let index = 0; index < blocks.length; index++) {
    const block = blocks[index];
    if (block.type === "definition") continue;
    const start = block.position?.start.offset;
    let end = block.position?.end.offset;
    if (start === undefined || end === undefined) continue;
    const next = blocks[index + 1];
    if (block.type === "heading" && next?.type === "paragraph" && !/^@(?:ds|ct)-/.test(normalized.text.slice(next.position?.start.offset))) {
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
