import { unified, type Processor } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";

export function createMarkdownParser<T>(parse: (source: string) => T, maxChars = 500_000, maxEntries = 3000) {
  const cache = new Map<string, T>();
  let chars = 0;
  return (source: string): T => {
    if (cache.has(source)) {
      const hit = cache.get(source)!;
      cache.delete(source);
      cache.set(source, hit);
      return hit;
    }
    const tree = parse(source);
    if (source.length > Math.min(50_000, maxChars)) return tree;
    cache.set(source, tree);
    chars += source.length;
    while (chars > maxChars || cache.size > maxEntries) {
      const oldest = cache.keys().next().value;
      if (oldest === undefined) break;
      chars -= oldest.length;
      cache.delete(oldest);
    }
    return tree;
  };
}

interface Point { line: number; column: number; offset?: number }
interface SyntaxNode { position?: { start: Point; end: Point }; children?: SyntaxNode[] }
interface SyntaxRoot extends SyntaxNode { type: "root"; children: SyntaxNode[] }

function shiftPositions(node: SyntaxNode, lines: number, offset: number): void {
  const shift = (point: Point): Point => point.offset === undefined
    ? { ...point, line: point.line + lines }
    : { ...point, line: point.line + lines, offset: point.offset + offset };
  if (node.position) node.position = { start: shift(node.position.start), end: shift(node.position.end) };
  node.children?.forEach((child) => shiftPositions(child, lines, offset));
}

function lineStart(source: string, offset: number): number {
  let index = offset;
  while (index > 0 && source[index - 1] !== "\n" && source[index - 1] !== "\r") index--;
  return index;
}

/**
 * Index of the first block that later text can still change. A root block parses alone like in context only after a blank
 * line (block starts depend on the block above) and once its first line is terminated (`2` may still become `2.` and join a list).
 */
function openBlockIndex(children: readonly SyntaxNode[], source: string): number {
  for (let index = children.length - 1; index > 0; index--) {
    const start = children[index].position?.start;
    const previousEnd = children[index - 1].position?.end;
    if (start?.offset === undefined || !previousEnd || start.line <= previousEnd.line + 1) continue;
    for (let offset = start.offset; offset < source.length; offset++) {
      if (source[offset] === "\n" || source[offset] === "\r") return index;
    }
  }
  return 0;
}

/**
 * Appending to a source re-parses only from its last open root block; earlier blocks are shared, so trees are immutable.
 * Reference definitions resolve across the whole document, so sources that may contain one always parse in full.
 */
export function createIncrementalMarkdownParser<T extends SyntaxRoot>(parse: (source: string) => T, maxStreams = 8) {
  const streams: Array<{ source: string; tree: T }> = [];
  return (source: string): T => {
    if (source.includes("]:")) return parse(source);
    let base = -1;
    for (let index = 0; index < streams.length; index++) {
      const candidate = streams[index].source;
      if (candidate.length <= source.length && source.startsWith(candidate) && (base < 0 || candidate.length > streams[base].source.length)) base = index;
    }
    const previous = base >= 0 ? streams.splice(base, 1)[0] : null;
    if (previous?.source.length === source.length) {
      streams.unshift(previous);
      return previous.tree;
    }
    const tree = previous ? extend(previous, source, parse) : parse(source);
    streams.unshift({ source, tree });
    if (streams.length > maxStreams) streams.length = maxStreams;
    return tree;
  };
}

function extend<T extends SyntaxRoot>({ source: previousSource, tree: previous }: { source: string; tree: T }, source: string, parse: (source: string) => T): T {
  const open = openBlockIndex(previous.children, previousSource);
  const start = previous.children[open]?.position?.start;
  if (open === 0 || start?.offset === undefined) return parse(source);
  const from = lineStart(source, start.offset);
  const tail = parse(source.slice(from));
  shiftPositions(tail, start.line - 1, from);
  return {
    ...tail,
    children: [...previous.children.slice(0, open), ...tail.children],
    position: { start: previous.position!.start, end: tail.position!.end },
  };
}

const parser = unified().use(remarkParse).use(remarkGfm);
type MarkdownRoot = ReturnType<typeof parser.parse>;
export const parsedMarkdown = createMarkdownParser(createIncrementalMarkdownParser((source: string) => parser.parse(source) as MarkdownRoot & SyntaxRoot));

export function remarkCachedParse(this: Processor) {
  // Shared trees are never mutated: remark transformers here copy the paths they change.
  this.parser = (source: string) => parsedMarkdown(source);
}
