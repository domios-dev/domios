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
interface SyntaxNode {
  type?: string;
  position?: { start: Point; end: Point };
  children?: SyntaxNode[];
  ordered?: boolean | null;
  start?: number | null;
  spread?: boolean | null;
}
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

function lineTerminated(source: string, offset: number): boolean {
  for (let index = offset; index < source.length; index++) {
    if (source[index] === "\n" || source[index] === "\r") return true;
  }
  return false;
}

function endsInCode(node: SyntaxNode): boolean {
  let last = node;
  while (last.children?.length) last = last.children[last.children.length - 1];
  return last.type === "code";
}

/**
 * Index of the first block that later text can still change. A root block parses alone like in context only after a blank
 * line that closed the block above and once its first line is terminated (`2` may become `2.`); code blocks carry flow state across blank lines.
 */
function openBlockIndex(children: readonly SyntaxNode[], source: string): number {
  for (let index = children.length - 1; index > 0; index--) {
    const start = children[index].position?.start;
    const previousEnd = children[index - 1].position?.end;
    if (start?.offset === undefined || !previousEnd || start.line <= previousEnd.line + 1) continue;
    if (children[index].type === "code" || endsInCode(children[index - 1])) continue;
    if (lineTerminated(source, start.offset)) return index;
  }
  return 0;
}

/** Index of the last list item whose marker line is terminated; the items before it are final. */
function openItemIndex(items: readonly SyntaxNode[], source: string): number {
  for (let index = items.length - 1; index > 0; index--) {
    const start = items[index].position?.start;
    if (start?.offset !== undefined && lineTerminated(source, start.offset)) return index;
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
  const block = previous.children[open];
  const item = block?.type === "list" ? openItemIndex(block.children!, previousSource) : 0;
  const start = (item > 0 ? block.children![item] : block)?.position?.start;
  if ((open === 0 && item === 0) || start?.offset === undefined) return parse(source);
  const from = lineStart(source, start.offset);
  const tail = parse(source.slice(from));
  shiftPositions(tail, start.line - 1, from);
  let children = tail.children;
  if (item > 0) {
    const list = tail.children[0];
    if (list?.type !== "list" || list.ordered !== block.ordered || !list.children?.length) return parse(source);
    const items = block.children!.slice(0, item);
    // A list is spread when a blank line separates any two items; boundaries before the re-parsed item are final.
    const gap = items.some((entry, index) => (items[index + 1] ?? list.children![0]).position!.start.line > entry.position!.end.line + 1);
    children = [
      { ...list, start: block.start, spread: gap || list.spread, children: [...items, ...list.children], position: { start: block.position!.start, end: list.position!.end } },
      ...tail.children.slice(1),
    ];
  }
  return {
    ...tail,
    children: [...previous.children.slice(0, open), ...children],
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
