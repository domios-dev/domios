import remarkGfm from "remark-gfm";
import remarkParse from "remark-parse";
import { unified } from "unified";

interface MarkdownNode {
  type: string;
  value?: string;
  alt?: string;
  children?: MarkdownNode[];
}

function renderChildren(node: MarkdownNode, separator = ""): string {
  return (node.children ?? []).map(markdownNodeText).join(separator);
}

function markdownNodeText(node: MarkdownNode): string {
  switch (node.type) {
    case "root":
    case "list":
    case "table":
      return renderChildren(node, "\n");
    case "paragraph":
    case "heading":
    case "blockquote":
    case "listItem":
    case "tableCell":
    case "link":
    case "delete":
    case "strong":
    case "emphasis":
      return renderChildren(node);
    case "tableRow":
      return renderChildren(node, " | ");
    case "text":
    case "inlineCode":
    case "code":
      return node.value ?? "";
    case "image":
      return node.alt ?? "";
    case "break":
      return "\n";
    default:
      return renderChildren(node);
  }
}

export function markdownToPlainText(text: string): string {
  const tree = unified().use(remarkParse).use(remarkGfm).parse(text) as MarkdownNode;
  return markdownNodeText(tree)
    .replace(/[ \t]+\n/g, "\n")
    .replace(/\n[ \t]+/g, "\n")
    .replace(/[ \t]{2,}/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}
