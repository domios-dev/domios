import { parseDomiosAttributes } from "./domios";
import { DsHtmlBoundary } from "./dsBlocks.html";

export type RawEnvelopeKind = "domios" | "pasted_content";
export interface RawEnvelopeExclusion {
  readonly start: number;
  readonly end: number;
  readonly kind: RawEnvelopeKind;
  readonly complete: boolean;
  readonly malformed: boolean;
}

const KINDS: readonly RawEnvelopeKind[] = ["domios", "pasted_content"];
const CONTEXTS = new Set(["context", "quoted-context", "quoted_context", "handoff", "provider-handoff", "provider_handoff", "environment_context", "system-reminder", "instructions", "INSTRUCTIONS", "previous_messages", "conversation_history", "tool_result", "tool_output", "transcript"]);
interface Opening { name: string; attrs: Record<string, string> | null; selfClosing: boolean }

function candidate(source: string, offset: number): RawEnvelopeKind | null {
  if (source[offset] !== "<") return null;
  for (const kind of KINDS) {
    const prefix = `<${kind}`;
    const tail = source.slice(offset, offset + prefix.length);
    if (tail.length < prefix.length && prefix.startsWith(tail)) return kind;
    if (tail === prefix && (offset + prefix.length === source.length || /[\t\r\n />]/.test(source[offset + prefix.length]))) return kind;
  }
  return null;
}

function opening(token: string): Opening | null {
  const match = /^<([A-Za-z][A-Za-z0-9_-]*)(?=[\t\r\n />])/.exec(token);
  if (!match || !token.endsWith(">")) return null;
  const selfClosing = token.endsWith("/>");
  const raw = token.slice(match[0].length, selfClosing ? -2 : -1);
  return { name: match[1], attrs: parseDomiosAttributes(raw.trimEnd()), selfClosing };
}

// Mirrors envelope/markup.rs quoted_end; fenced and inline code cannot terminate a frame.
function quotedEnd(source: string, offset: number): number | null {
  if (offset === 0 || source[offset - 1] === "\n") {
    const newline = source.indexOf("\n", offset);
    const lineEnd = newline < 0 ? source.length : newline + 1;
    const line = source.slice(offset, lineEnd);
    if (/^(?: {4}|\t| {0,3}>)/.test(line)) return lineEnd;
    const fence = /^ {0,3}(`{3,}|~{3,})/.exec(line);
    if (fence) {
      const marker = fence[1][0];
      let cursor = lineEnd;
      while (cursor < source.length) {
        const next = source.indexOf("\n", cursor);
        const end = next < 0 ? source.length : next + 1;
        const close = source.slice(cursor, end).trim();
        if (close.length >= fence[1].length && [...close].every((char) => char === marker)) return end;
        cursor = end;
      }
      return source.length;
    }
  }
  if (source[offset] !== "`") return null;
  let count = 1;
  while (source[offset + count] === "`") count++;
  let cursor = offset + count;
  while (cursor < source.length) {
    const next = source.indexOf("`", cursor);
    if (next < 0) break;
    let end = next + 1;
    while (source[end] === "`") end++;
    if (end - next === count) return end;
    cursor = end;
  }
  return source.length;
}

function extent(source: string, start: number, kind: RawEnvelopeKind): RawEnvelopeExclusion {
  const lexer = new DsHtmlBoundary("raw-exclusion-boundary");
  const stack: Opening[] = [];
  const contexts: string[] = [];
  let tokenStart = start;
  let malformed = false;
  for (let cursor = start; cursor < source.length; cursor++) {
    if (lexer.inData && cursor !== start) {
      const quoted = quotedEnd(source, cursor);
      if (quoted !== null) { cursor = quoted - 1; continue; }
    }
    if (lexer.inData) tokenStart = cursor;
    lexer.feed(source[cursor]);
    if (!lexer.inData || source[tokenStart] !== "<") continue;
    const token = source.slice(tokenStart, cursor + 1);
    const opened = opening(token);
    if (opened) {
      if (CONTEXTS.has(opened.name)) {
        if (!opened.selfClosing) contexts.push(opened.name);
      } else if (!contexts.length && KINDS.includes(opened.name as RawEnvelopeKind)) {
        malformed ||= opened.attrs === null;
        if (!opened.selfClosing) stack.push(opened);
        else if (tokenStart === start) return { start, end: cursor + 1, kind, complete: true, malformed };
      }
      continue;
    }
    if (!token.startsWith("</") || !token.endsWith(">")) continue;
    if (contexts.length) {
      if (token === `</${contexts[contexts.length - 1]}>`) contexts.pop();
      continue;
    }
    const active = stack[stack.length - 1];
    if (!active) continue;
    let closes = token === `</${active.name}>`;
    if (active.name === "pasted_content" && token.startsWith("</pasted_content") && /[\t\r\n >]/.test(token["</pasted_content".length] ?? "")) {
      const attrs = parseDomiosAttributes(token.slice("</pasted_content".length, -1).trimEnd());
      closes = attrs !== null && (attrs.id === undefined || attrs.id === active.attrs?.id);
      if (!closes) malformed = true;
    }
    if (!closes && token.startsWith(`</${active.name}`)) malformed = true;
    if (closes) {
      stack.pop();
      if (!stack.length) return { start, end: cursor + 1, kind, complete: true, malformed };
    }
  }
  return { start, end: source.length, kind, complete: false, malformed };
}

/** Raw UTF-16 exclusion ranges only. Source bytes never authenticate a sender or authorize effects. */
export function rawEnvelopeExclusions(source: string): readonly RawEnvelopeExclusion[] {
  const spans: RawEnvelopeExclusion[] = [];
  for (let cursor = 0; cursor < source.length; cursor++) {
    const kind = candidate(source, cursor);
    if (kind) {
      const span = extent(source, cursor, kind);
      spans.push(span);
      cursor = span.end - 1;
    }
  }
  return spans;
}
