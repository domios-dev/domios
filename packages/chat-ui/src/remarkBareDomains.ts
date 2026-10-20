import { matchBareDomains } from "./bareDomains";

interface MarkdownNode {
  type: string;
  value?: string;
  url?: string;
  children?: MarkdownNode[];
  data?: { hProperties?: Record<string, unknown> };
  position?: { start: Point; end: Point };
}
interface Point { line: number; column: number; offset?: number }

function slicePosition(node: MarkdownNode, start: number, end: number): MarkdownNode["position"] {
  if (!node.position) return undefined;
  const point = (offset: number): Point => {
    const lines = (node.value ?? "").slice(0, offset).split("\n");
    return {
      line: node.position!.start.line + lines.length - 1,
      column: lines.length === 1 ? node.position!.start.column + offset : lines[lines.length - 1].length + 1,
      offset: node.position!.start.offset === undefined ? undefined : node.position!.start.offset + offset,
    };
  };
  return { start: point(start), end: point(end) };
}

function linkedText(node: MarkdownNode): MarkdownNode[] {
  const value = node.value ?? "";
  const matches = matchBareDomains(value);
  if (!matches.length) return [node];
  const result: MarkdownNode[] = [];
  let cursor = 0;
  const textNode = (start: number, end: number): MarkdownNode => ({ type: "text", value: value.slice(start, end), position: slicePosition(node, start, end) });
  for (const match of matches) {
    if (cursor < match.start) result.push(textNode(cursor, match.start));
    result.push({ type: "link", url: match.url, position: slicePosition(node, match.start, match.end), children: [textNode(match.start, match.end)] });
    cursor = match.end;
  }
  if (cursor < value.length) result.push(textNode(cursor, value.length));
  return result;
}

// Parsed trees are shared between renders, so transformers copy the paths they change instead of mutating.
function withChildren(node: MarkdownNode, children: MarkdownNode[]): MarkdownNode {
  return children.length === node.children!.length && children.every((child, index) => child === node.children![index])
    ? node
    : { ...node, children };
}

export function remarkBareDomains() {
  return (tree: MarkdownNode) => {
    function walk(node: MarkdownNode): MarkdownNode {
      if (["link", "linkReference", "code", "inlineCode"].includes(node.type) || !node.children) return node;
      let rawProtected = 0;
      return withChildren(node, node.children.flatMap((child) => {
        if (child.type === "html") {
          for (const tag of (child.value ?? "").matchAll(/<(\/)?(?:a|code|pre)\b[^>]*>/gi)) rawProtected += tag[1] ? -1 : 1;
          return [child];
        }
        if (rawProtected > 0) return [child];
        if (child.type === "text") return linkedText(child);
        return [walk(child)];
      }));
    }
    return walk(tree);
  };
}

export function remarkCodeMetadata() {
  return (tree: MarkdownNode) => {
    function walk(node: MarkdownNode): MarkdownNode {
      if (node.type === "code" || node.type === "inlineCode") {
        return { ...node, data: { ...node.data, hProperties: { ...node.data?.hProperties, "data-code-text": node.value ?? "", "data-code-block": node.type === "code" } } };
      }
      return node.children ? withChildren(node, node.children.map(walk)) : node;
    }
    return walk(tree);
  };
}
