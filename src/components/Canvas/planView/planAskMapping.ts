export type PlanDocumentKind = "plan" | "spec";
export type PlanAskMode = "paragraph" | "selection";

export const PLAN_ASK_BODY_SELECTOR = "[data-plan-ask-body]";
export const PLAN_ASK_BLOCK_SELECTOR = "p, li, h1, h2, h3, h4, h5, h6, blockquote, td, th";

const TEXT_NODE = 3;
const DOCUMENT_POSITION_PRECEDING = 2;
const DOCUMENT_POSITION_FOLLOWING = 4;
// Source characters Markdown drops from rendered text: line-end spaces, escapes, inline-code fences.
const CONSUMED_SOURCE = /[ \t\\`]/;

export interface OffsetRange {
  start: number;
  end: number;
}

export interface PlanAskSource {
  id: string;
  title: string;
  body: string;
}

export interface PlanAskContext {
  planId: string;
  document: PlanDocumentKind;
  specId: string | null;
  section: (id: string) => PlanAskSource | null;
}

/** Offsets are UTF-16 code units: `source` indexes the section body, `rendered` its visible text. */
export interface PlanAskReference {
  planId: string;
  document: PlanDocumentKind;
  specId: string | null;
  sectionId: string;
  sectionTitle: string;
  mode: PlanAskMode;
  quote: string;
  source: OffsetRange;
  rendered: OffsetRange;
}

export type PlanAskTarget =
  | { status: "mapped"; mode: PlanAskMode; reference: PlanAskReference; block: HTMLElement | null }
  | { status: "unmapped"; mode: PlanAskMode; quote: string; block: HTMLElement | null };

export interface RangeLike {
  startContainer: Node;
  startOffset: number;
  endContainer: Node;
  endOffset: number;
  toString(): string;
}

interface TextPoint {
  index: number;
  offset: number;
}

function parseRange(value: string | null | undefined): OffsetRange | null {
  const match = value ? /^(\d+)-(\d+)$/.exec(value) : null;
  if (!match) return null;
  const start = Number(match[1]);
  const end = Number(match[2]);
  return end >= start ? { start, end } : null;
}

function elementSource(element: Element): OffsetRange | null {
  return parseRange(`${element.getAttribute("data-src-start") ?? ""}-${element.getAttribute("data-src-end") ?? ""}`);
}

function directTextNodes(element: Element): Text[] {
  return Array.from(element.childNodes).filter((node): node is Text => node.nodeType === TEXT_NODE);
}

function textSpan(node: Text): OffsetRange | null {
  const parent = node.parentElement;
  const encoded = parent?.getAttribute("data-src-text");
  if (!parent || !encoded) return null;
  const spans = encoded.split(",");
  const siblings = directTextNodes(parent);
  if (spans.length !== siblings.length) return null;
  return parseRange(spans[siblings.indexOf(node)]);
}

/** Maps each rendered character to its source index (plus the end index), or null when the text is not a clean projection of the source. */
export function alignRenderedToSource(rendered: string, source: string): number[] | null {
  const map: number[] = [];
  let j = 0;
  for (let i = 0; i < rendered.length; i++) {
    while (j < source.length && source[j] !== rendered[i] && CONSUMED_SOURCE.test(source[j])) j++;
    if (source[j] !== rendered[i]) return null;
    map.push(j);
    j++;
  }
  map.push(j);
  for (; j < source.length; j++) {
    if (!CONSUMED_SOURCE.test(source[j])) return null;
  }
  return map;
}

function sourcePoint(node: Text, offset: number, edge: "start" | "end", body: string): number | null {
  const span = textSpan(node);
  if (!span || span.end > body.length) return null;
  const map = alignRenderedToSource(node.data, body.slice(span.start, span.end));
  if (!map) return null;
  if (edge === "end" && offset > 0) return span.start + map[offset - 1] + 1;
  return span.start + map[Math.min(offset, node.data.length)];
}

function textNodesIn(root: Node): Text[] {
  const out: Text[] = [];
  const visit = (node: Node) => {
    if (node.nodeType === TEXT_NODE) out.push(node as Text);
    node.childNodes.forEach(visit);
  };
  root.childNodes.forEach(visit);
  return out;
}

function follows(reference: Node, node: Node): boolean {
  return (reference.compareDocumentPosition(node) & DOCUMENT_POSITION_FOLLOWING) !== 0;
}

function precedes(reference: Node, node: Node): boolean {
  return (reference.compareDocumentPosition(node) & DOCUMENT_POSITION_PRECEDING) !== 0;
}

function locate(texts: Text[], container: Node, offset: number, edge: "start" | "end"): TextPoint | null {
  if (container.nodeType === TEXT_NODE) {
    const index = texts.indexOf(container as Text);
    return index < 0 ? null : { index, offset };
  }
  const child = container.childNodes[offset] ?? null;
  if (edge === "start") {
    const index = texts.findIndex((text) =>
      child
        ? child === text || child.contains(text) || follows(child, text)
        : !container.contains(text) && follows(container, text),
    );
    return index < 0 ? null : { index, offset: 0 };
  }
  let index = -1;
  texts.forEach((text, i) => {
    const before = child
      ? child !== text && !child.contains(text) && precedes(child, text)
      : container.contains(text) || precedes(container, text);
    if (before) index = i;
  });
  return index < 0 ? null : { index, offset: texts[index].data.length };
}

function snapPastLayoutWhitespace(texts: Text[], point: TextPoint, edge: "start" | "end"): TextPoint | null {
  let { index, offset } = point;
  while (index >= 0 && index < texts.length && !textSpan(texts[index]) && !texts[index].data.trim()) {
    index += edge === "start" ? 1 : -1;
    offset = edge === "start" || index < 0 ? 0 : texts[index].data.length;
  }
  return index >= 0 && index < texts.length ? { index, offset } : null;
}

function renderedOffset(texts: Text[], point: TextPoint): number {
  let total = point.offset;
  for (let i = 0; i < point.index; i++) total += texts[i].data.length;
  return total;
}

function elementOf(node: Node): Element | null {
  return node.nodeType === TEXT_NODE ? node.parentElement : (node as Element);
}

function bodyOf(node: Node): HTMLElement | null {
  return elementOf(node)?.closest?.<HTMLElement>(PLAN_ASK_BODY_SELECTOR) ?? null;
}

function sectionFor(body: Element, context: PlanAskContext): PlanAskSource | null {
  return context.section(body.getAttribute("data-section-id") ?? "");
}

function reference(
  context: PlanAskContext,
  section: PlanAskSource,
  mode: PlanAskMode,
  quote: string,
  source: OffsetRange,
  rendered: OffsetRange,
): PlanAskReference {
  return {
    planId: context.planId,
    document: context.document,
    specId: context.specId,
    sectionId: section.id,
    sectionTitle: section.title,
    mode,
    quote,
    source,
    rendered,
  };
}

export function paragraphTarget(block: HTMLElement, context: PlanAskContext): PlanAskTarget {
  const quote = block.textContent ?? "";
  const unmapped: PlanAskTarget = { status: "unmapped", mode: "paragraph", quote, block };
  const body = bodyOf(block);
  const section = body ? sectionFor(body, context) : null;
  const source = elementSource(block);
  if (!body || !section || !source || source.end > section.body.length || !quote.trim()) return unmapped;
  const texts = textNodesIn(body);
  const first = textNodesIn(block)[0];
  const index = first ? texts.indexOf(first) : -1;
  if (index < 0) return unmapped;
  const start = renderedOffset(texts, { index, offset: 0 });
  return {
    status: "mapped",
    mode: "paragraph",
    block,
    reference: reference(context, section, "paragraph", quote, source, { start, end: start + quote.length }),
  };
}

/** Null when the selection is empty or outside the reader; unmapped when it cannot be traced to exact source offsets. */
export function selectionTarget(range: RangeLike, root: Element, context: PlanAskContext): PlanAskTarget | null {
  const quote = range.toString();
  if (!quote.trim()) return null;
  const startBody = bodyOf(range.startContainer);
  const endBody = bodyOf(range.endContainer);
  if (!startBody || !endBody || !root.contains(startBody) || !root.contains(endBody)) return null;
  const block = elementOf(range.startContainer)?.closest?.<HTMLElement>(PLAN_ASK_BLOCK_SELECTOR) ?? startBody;
  const unmapped: PlanAskTarget = { status: "unmapped", mode: "selection", quote, block };
  const section = startBody === endBody ? sectionFor(startBody, context) : null;
  if (!section) return unmapped;
  const texts = textNodesIn(startBody);
  const startPoint = locate(texts, range.startContainer, range.startOffset, "start");
  const endPoint = locate(texts, range.endContainer, range.endOffset, "end");
  const start = startPoint && snapPastLayoutWhitespace(texts, startPoint, "start");
  const end = endPoint && snapPastLayoutWhitespace(texts, endPoint, "end");
  if (!start || !end) return unmapped;
  const sourceStart = sourcePoint(texts[start.index], start.offset, "start", section.body);
  const sourceEnd = sourcePoint(texts[end.index], end.offset, "end", section.body);
  if (sourceStart === null || sourceEnd === null || sourceEnd < sourceStart) return unmapped;
  return {
    status: "mapped",
    mode: "selection",
    block,
    reference: reference(
      context,
      section,
      "selection",
      quote,
      { start: sourceStart, end: sourceEnd },
      { start: renderedOffset(texts, start), end: renderedOffset(texts, end) },
    ),
  };
}
