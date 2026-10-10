interface HastPoint {
  offset?: number;
}

export interface HastNode {
  type: string;
  tagName?: string;
  position?: { start?: HastPoint; end?: HastPoint };
  properties?: Record<string, unknown>;
  children?: HastNode[];
}

function span(node: HastNode): { start: number; end: number } | null {
  const start = node.position?.start?.offset;
  const end = node.position?.end?.offset;
  return typeof start === "number" && typeof end === "number" ? { start, end } : null;
}

function annotate(node: HastNode, offsets?: { start: (offset: number) => number; end: (offset: number) => number }): void {
  if (node.type === "element") {
    const properties = (node.properties ??= {});
    const own = span(node);
    if (own) {
      properties.dataSrcStart = String(offsets?.start(own.start) ?? own.start);
      properties.dataSrcEnd = String(offsets?.end(own.end) ?? own.end);
    }
    const texts = (node.children ?? []).filter((child) => child.type === "text");
    if (texts.length > 0) {
      properties.dataSrcText = texts
        .map((child) => {
          const textSpan = span(child);
          return textSpan ? `${offsets?.start(textSpan.start) ?? textSpan.start}-${offsets?.end(textSpan.end) ?? textSpan.end}` : "_";
        })
        .join(",");
    }
  }
  node.children?.forEach((child) => annotate(child, offsets));
}

/** Stamps source offsets on elements (`data-src-start/end`) and their direct text children (`data-src-text`, `_` when unknown). */
export function rehypeSourceOffsets(offsets?: { start: (offset: number) => number; end: (offset: number) => number }) {
  return (tree: HastNode) => {
    annotate(tree, offsets);
  };
}

export function rehypeMessageParts(parts: readonly import("./messageParts").MessagePart[]) {
  return () => (tree: HastNode) => {
    const groups = new Map<string, HastNode>();
    tree.children = (tree.children ?? []).map((node) => {
      const offset = node.position?.start?.offset;
      const part = offset === undefined ? undefined : parts.find((candidate) => offset >= candidate.renderedStart && offset < candidate.renderedEnd);
      if (!part) return node;
      const existing = groups.get(part.id);
      if (existing) { existing.children?.push(node); return null; }
      const wrapper: HastNode = {
        type: "element", tagName: "div",
        properties: { dataMessagePartId: part.id, dataSrcStart: String(part.start), dataSrcEnd: String(part.end), className: ["ct-message-part"] },
        children: [node],
      };
      groups.set(part.id, wrapper);
      return wrapper;
    }).filter((node): node is HastNode => node !== null);
  };
}
