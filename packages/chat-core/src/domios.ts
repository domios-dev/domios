export const DOMIOS_CLOSE_TAG = "</domios>";
export const DOMIOS_TYPED_INSTRUCTION = "Handle the message above.";

export interface ParsedDomiosMessage {
  attrs: Record<string, string>;
  content: string;
  body: string;
}

export function buildDomiosOpeningTag(attrs: Record<string, string>): string {
  const serialized = Object.entries(attrs)
    .map(([name, value]) => ` ${name}="${escapeAttribute(value)}"`)
    .join("");
  return `<domios${serialized}>`;
}

export function buildDomiosMessage(
  attrs: Record<string, string>,
  body: string,
  suffix = "",
): string {
  return `${buildDomiosOpeningTag(attrs)}${body}${suffix}${DOMIOS_CLOSE_TAG}`;
}

export function parseDomiosMessage(text: string): ParsedDomiosMessage | null {
  if (!text.startsWith("<domios")) return null;
  const openEnd = text.indexOf(">");
  if (openEnd < 0) return null;
  const attrs = parseAttributes(text.slice("<domios".length, openEnd));
  if (!attrs || !attrs.from) return null;
  const content = text.slice(openEnd + 1);
  if (!content.endsWith(DOMIOS_CLOSE_TAG)) return null;
  const enclosed = content.slice(0, -DOMIOS_CLOSE_TAG.length);
  return { attrs, content: enclosed, body: stripTypedSuffix(enclosed) };
}

function parseAttributes(source: string): Record<string, string> | null {
  const attrs: Record<string, string> = {};
  let rest = source;
  const attribute = /^\s+([A-Za-z][A-Za-z0-9_]*)="([^"]*)"/;
  while (rest.length > 0) {
    const match = attribute.exec(rest);
    if (!match) return null;
    const [, name, raw] = match;
    if (Object.prototype.hasOwnProperty.call(attrs, name)) return null;
    const decoded = decodeAttribute(raw);
    if (decoded === null) return null;
    attrs[name] = decoded;
    rest = rest.slice(match[0].length);
  }
  return attrs;
}

function escapeAttribute(value: string): string {
  return value.replace(/[&"<>\u0000-\u001f\u007f-\u009f\u2028\u2029]/g, (char) => {
    switch (char) {
      case "&": return "&amp;";
      case '"': return "&quot;";
      case "<": return "&lt;";
      case ">": return "&gt;";
      default: return " ";
    }
  });
}

function decodeAttribute(value: string): string | null {
  let decoded = "";
  let remaining = value;
  while (remaining.length > 0) {
    const ampersand = remaining.indexOf("&");
    if (ampersand < 0) return decoded + remaining;
    decoded += remaining.slice(0, ampersand);
    const entity = ["&amp;", "&quot;", "&lt;", "&gt;"].find((candidate) =>
      remaining.startsWith(candidate, ampersand),
    );
    if (!entity) return null;
    decoded += entity === "&amp;" ? "&" : entity === "&quot;" ? '"' : entity === "&lt;" ? "<" : ">";
    remaining = remaining.slice(ampersand + entity.length);
  }
  return decoded;
}

function stripTypedSuffix(content: string): string {
  let body = content;
  const instruction = ` ${DOMIOS_TYPED_INSTRUCTION}`;
  if (body.endsWith(instruction)) body = body.slice(0, -instruction.length);
  const replyStarts = [" [to reply, run: codeterm send ", " [sender is a shell; reply here]"];
  const start = replyStarts
    .map((token) => body.lastIndexOf(token))
    .filter((index) => index >= 0)
    .sort((left, right) => right - left)[0];
  return start === undefined ? body : body.slice(0, start);
}
