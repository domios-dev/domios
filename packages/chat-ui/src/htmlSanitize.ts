import { defaultSchema } from "rehype-sanitize";

type HastNode = {
  type?: string;
  tagName?: string;
  properties?: Record<string, unknown>;
  children?: HastNode[];
};

const EXTRA_TAGS = [
  "abbr",
  "caption",
  "col",
  "colgroup",
  "dd",
  "details",
  "dfn",
  "div",
  "dl",
  "dt",
  "figcaption",
  "figure",
  "kbd",
  "mark",
  "s",
  "samp",
  "small",
  "span",
  "summary",
  "sub",
  "sup",
  "time",
  "u",
  "var",
  "wbr",
];

const SAFE_SVG_TAGS = ["circle", "ellipse", "g", "line", "path", "polygon", "polyline", "rect", "svg"];
const SVG_PAINT_ATTRIBUTES = new Set(["color", "fill", "stroke"]);
const SAFE_SVG_PAINT = /^(?:none|currentcolor|transparent|inherit|[a-z]+|#[0-9a-f]{3,8}|(?:rgb|rgba|hsl|hsla)\([^()]*)$/i;

/** Cosmetic properties only: nothing that can position, overlay, or animate out of flow. */
const ALLOWED_STYLE_PROPERTIES = new Set([
  "align-items", "background", "background-color", "background-image", "border", "border-bottom",
  "border-collapse", "border-color", "border-left", "border-radius", "border-right", "border-spacing",
  "border-style", "border-top", "border-width", "box-shadow", "color", "column-gap", "display", "filter",
  "flex", "flex-basis", "flex-direction", "flex-grow", "flex-shrink", "flex-wrap", "font", "font-family",
  "font-size", "font-style", "font-variant", "font-weight", "gap", "grid-column", "grid-row",
  "grid-template-columns", "height", "justify-content", "letter-spacing", "line-height", "list-style",
  "list-style-type", "margin", "margin-bottom", "margin-left", "margin-right", "margin-top", "max-height",
  "max-width", "min-height", "min-width", "opacity", "outline", "overflow-wrap", "padding",
  "padding-bottom", "padding-left", "padding-right", "padding-top", "row-gap", "text-align",
  "text-decoration", "text-indent", "text-shadow", "text-transform", "vertical-align", "white-space",
  "width", "word-break", "word-spacing",
]);

/** Escapes and comments are how a denylist gets walked around, so both fail the check outright. */
const STYLE_VALUE_DENY = /(url\s*\(|expression\s*\(|javascript:|@import|\\|<)/i;

function stripCssComments(value: string): string {
  return value.replace(/\/\*[\s\S]*?\*\//g, " ");
}

export const CHAT_HTML_SANITIZE_SCHEMA = {
  ...defaultSchema,
  tagNames: [...new Set([...(defaultSchema.tagNames ?? []), ...EXTRA_TAGS, ...SAFE_SVG_TAGS])],
  attributes: {
    ...defaultSchema.attributes,
    "*": [...new Set([...(defaultSchema.attributes?.["*"] ?? []), "className", "style", "title"])],
    td: [...new Set([...(defaultSchema.attributes?.td ?? []), "colSpan", "rowSpan"])],
    th: [...new Set([...(defaultSchema.attributes?.th ?? []), "colSpan", "rowSpan", "scope"])],
    details: ["open"],
    abbr: ["title"],
    time: ["dateTime"],
    img: [...new Set([...(defaultSchema.attributes?.img ?? []), "loading", "width", "height"])],
    svg: ["ariaHidden", "color", "fill", "height", "preserveAspectRatio", "role", "stroke", "strokeLinecap", "strokeLinejoin", "strokeWidth", "viewBox", "width"],
    g: ["color", "fill", "stroke", "strokeLinecap", "strokeLinejoin", "strokeWidth"],
    path: ["clipRule", "d", "fill", "fillRule", "stroke", "strokeLinecap", "strokeLinejoin", "strokeWidth"],
    circle: ["cx", "cy", "fill", "r", "stroke", "strokeWidth"],
    ellipse: ["cx", "cy", "fill", "rx", "ry", "stroke", "strokeWidth"],
    line: ["stroke", "strokeLinecap", "strokeWidth", "x1", "x2", "y1", "y2"],
    polygon: ["fill", "points", "stroke", "strokeLinejoin", "strokeWidth"],
    polyline: ["fill", "points", "stroke", "strokeLinecap", "strokeLinejoin", "strokeWidth"],
    rect: ["fill", "height", "rx", "ry", "stroke", "strokeWidth", "width", "x", "y"],
  },
  protocols: {
    ...defaultSchema.protocols,
    src: [...new Set([...(defaultSchema.protocols?.src ?? []), "data"])],
  },
};

export function sanitizeStyleValue(value: string): string | null {
  const declarations = stripCssComments(value)
    .split(";")
    .map((part) => part.trim())
    .filter((part) => part !== "")
    .filter((part) => {
      const separator = part.indexOf(":");
      if (separator < 1) return false;
      const property = part.slice(0, separator).trim().toLowerCase();
      const declared = part.slice(separator + 1).trim();
      return ALLOWED_STYLE_PROPERTIES.has(property) && declared !== "" && !STYLE_VALUE_DENY.test(declared);
    });
  return declarations.length ? `${declarations.join("; ")};` : null;
}

function scrubNode(node: HastNode) {
  const style = node.properties?.style;
  if (typeof style === "string") {
    const safe = sanitizeStyleValue(style);
    if (safe) {
      node.properties!.style = safe;
    } else {
      delete node.properties!.style;
    }
  }
  if (SAFE_SVG_TAGS.includes(node.tagName ?? "") && node.properties) {
    for (const [name, value] of Object.entries(node.properties)) {
      if (SVG_PAINT_ATTRIBUTES.has(name) && (typeof value !== "string" || !SAFE_SVG_PAINT.test(value.trim()))) {
        delete node.properties[name];
      }
    }
  }
  for (const child of node.children ?? []) {
    scrubNode(child);
  }
}

/** Runs after rehype-sanitize, which passes `style` through as an opaque string. */
export function rehypeScrubInlineStyles() {
  return (tree: HastNode) => {
    scrubNode(tree);
  };
}

export function containsHtmlMarkup(text: string): boolean {
  return /<[a-zA-Z][a-zA-Z0-9-]*(\s|\/?>)|<\/[a-zA-Z]/.test(text);
}

const MARKDOWN_HTML_BLOCK_TAGS = new Set([
  "address", "article", "aside", "blockquote", "details", "dialog", "div", "dl", "fieldset",
  "figure", "footer", "form", "header", "main", "nav", "ol", "p", "section", "table", "ul",
]);
const HTML_TAG_PATTERN = /<\/?([a-z][a-z0-9-]*)\b(?:[^"'<>]|"[^"]*"|'[^']*')*>/gi;
const HTML_WHITESPACE_SENSITIVE_TAG = /<(?:pre|script|style|textarea)\b/i;

type HtmlTagToken = {
  start: number;
  end: number;
  name: string;
  closing: boolean;
  selfClosing: boolean;
};

function fencedCodeRanges(text: string): Array<[number, number]> {
  const ranges: Array<[number, number]> = [];
  const lines = text.matchAll(/^.*(?:\r?\n|$)/gm);
  let open: { start: number; marker: string; length: number } | null = null;
  for (const match of lines) {
    const line = match[0].replace(/\r?\n$/, "");
    if (!open) {
      const fence = /^ {0,3}(`{3,}|~{3,})/.exec(line);
      if (fence) open = { start: match.index, marker: fence[1][0], length: fence[1].length };
      continue;
    }
    const closing = new RegExp(`^ {0,3}${open.marker === "`" ? "`" : "~"}{${open.length},}\\s*$`);
    if (closing.test(line)) {
      ranges.push([open.start, match.index + match[0].length]);
      open = null;
    }
  }
  if (open) ranges.push([open.start, text.length]);
  return ranges;
}

function htmlTagTokens(text: string): HtmlTagToken[] {
  return [...text.matchAll(HTML_TAG_PATTERN)].map((match) => ({
    start: match.index,
    end: match.index + match[0].length,
    name: match[1].toLowerCase(),
    closing: match[0].startsWith("</"),
    selfClosing: /\/\s*>$/.test(match[0]),
  }));
}

function startsMarkdownHtmlBlock(text: string, token: HtmlTagToken, fenced: Array<[number, number]>): boolean {
  if (token.closing || token.selfClosing || !MARKDOWN_HTML_BLOCK_TAGS.has(token.name)) return false;
  if (fenced.some(([start, end]) => token.start >= start && token.start < end)) return false;
  const lineStart = text.lastIndexOf("\n", token.start - 1) + 1;
  const indentation = text.slice(lineStart, token.start);
  return indentation.trim() === "" && indentation.length <= 3;
}

/** Keep blank lines inside raw block HTML from terminating the CommonMark HTML block. */
export function normalizeMultilineHtmlBlocks(text: string): string {
  if (!containsHtmlMarkup(text) || !/\r?\n[ \t]*\r?\n/.test(text)) return text;
  const fenced = fencedCodeRanges(text);
  const tokens = htmlTagTokens(text);
  let cursor = 0;
  let output = "";

  for (let i = 0; i < tokens.length; i += 1) {
    const root = tokens[i];
    if (root.start < cursor || !startsMarkdownHtmlBlock(text, root, fenced)) continue;
    let depth = 1;
    let closingIndex = -1;
    for (let j = i + 1; j < tokens.length; j += 1) {
      const token = tokens[j];
      if (token.name !== root.name || token.selfClosing) continue;
      depth += token.closing ? -1 : 1;
      if (depth === 0) {
        closingIndex = j;
        break;
      }
    }
    if (closingIndex < 0) continue;

    const end = tokens[closingIndex].end;
    const block = text.slice(root.start, end);
    if (HTML_WHITESPACE_SENSITIVE_TAG.test(block)) continue;
    const normalized = block.replace(/\r?\n[ \t]*\r?\n/g, "\n");
    if (normalized === block) continue;
    output += text.slice(cursor, root.start) + normalized;
    cursor = end;
    i = closingIndex;
  }

  return cursor === 0 ? text : output + text.slice(cursor);
}
