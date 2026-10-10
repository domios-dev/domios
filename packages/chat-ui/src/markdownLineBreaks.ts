interface HastNode {
  type: string;
  tagName?: string;
  value?: string;
  position?: unknown;
  children?: HastNode[];
}

function removeGeneratedBreakWhitespace(node: HastNode): void {
  if (!node.children) return;
  // Markdown emits a formatting newline after <br>; pre-wrap would display it twice.
  node.children = node.children.filter((child, index, children) => !(
    children[index - 1]?.tagName === "br" && child.type === "text" && !child.position && child.value === "\n"
  ));
  node.children.forEach(removeGeneratedBreakWhitespace);
}

export function rehypeMarkdownLineBreaks() {
  return removeGeneratedBreakWhitespace;
}
