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
  tagNames: [...new Set([...(defaultSchema.tagNames ?? []), ...EXTRA_TAGS])],
  attributes: {
    ...defaultSchema.attributes,
    "*": [...new Set([...(defaultSchema.attributes?.["*"] ?? []), "className", "style", "title"])],
    td: [...new Set([...(defaultSchema.attributes?.td ?? []), "colSpan", "rowSpan"])],
    th: [...new Set([...(defaultSchema.attributes?.th ?? []), "colSpan", "rowSpan", "scope"])],
    details: ["open"],
    abbr: ["title"],
    time: ["dateTime"],
    img: [...new Set([...(defaultSchema.attributes?.img ?? []), "loading", "width", "height"])],
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
