interface HastPoint {
  offset?: number;
}

interface HastNode {
  type: string;
  position?: { start?: HastPoint; end?: HastPoint };
  properties?: Record<string, unknown>;
  children?: HastNode[];
}

function span(node: HastNode): { start: number; end: number } | null {
  const start = node.position?.start?.offset;
  const end = node.position?.end?.offset;
  return typeof start === "number" && typeof end === "number" ? { start, end } : null;
}

function annotate(node: HastNode): void {
  if (node.type === "element") {
    const properties = (node.properties ??= {});
    const own = span(node);
    if (own) {
      properties.dataSrcStart = String(own.start);
      properties.dataSrcEnd = String(own.end);
    }
    const texts = (node.children ?? []).filter((child) => child.type === "text");
    if (texts.length > 0) {
      properties.dataSrcText = texts
        .map((child) => {
          const textSpan = span(child);
          return textSpan ? `${textSpan.start}-${textSpan.end}` : "_";
        })
        .join(",");
    }
  }
  node.children?.forEach(annotate);
}

/** Stamps source offsets on elements (`data-src-start/end`) and their direct text children (`data-src-text`, `_` when unknown). */
export function rehypeSourceOffsets() {
  return (tree: HastNode) => {
    annotate(tree);
  };
}
