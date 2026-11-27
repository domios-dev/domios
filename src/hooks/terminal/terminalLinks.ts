import type { IBufferCell, ILink, ILinkProvider, IBufferRange, Terminal } from "@xterm/xterm";
import { nativeBridge } from "../../lib/nativeBridge";

const URL_START_RE = /https?:\/\/[^\s"'<>`│╭╮╰╯─]+/gi;
const URL_TOKEN_RE = /^[^\s"'<>`│╭╮╰╯─]+/;
const TRAILING_PUNCTUATION = new Set([".", ",", ";", ":", "!", "?"]);
const HARD_WRAP_CONTINUATION_TAIL = /[-_&=+%/?:~.]$/;
const HARD_WRAP_URL_STRUCTURE = /[&=%/+?#_-]/;
const HARD_WRAP_FORBIDDEN_START = /^(?:https?:\/\/|[$>#•*│├└─])/;
const HARD_WRAP_SENTENCE_WORD = /^[A-Z][a-z]{1,20}[.!?]?$/;
const MAX_JOINED_LINES = 24;
const MAX_JOINED_CHARS = 4096;
const MIN_HARD_WRAP_LINE_CHARS = 50;
const MIN_HARD_WRAP_TAIL_URL_CHARS = 28;
const MAX_HARD_WRAP_INDENT = 8;

export interface TerminalLinkContext {
  isRemoteTab: boolean;
}

export type TerminalLinkDecision =
  | { action: "open_local"; url: string }
  | { action: "refuse"; reason: "invalid_url" | "disallowed_scheme" };

/** Local and remote terminal links share the same strict http/https allowlist. */
export function resolveSafeTerminalLinkUrl(raw: string): string | null {
  const trimmed = raw.trim();
  if (!trimmed || /[\u0000-\u001f\u007f\s]/.test(trimmed)) return null;
  let parsed: URL;
  try {
    parsed = new URL(trimmed);
  } catch {
    return null;
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
  if (!parsed.host) return null;
  return trimmed;
}

/** Terminal links always open on the viewing machine, never on a remote host. */
export function resolveTerminalLinkTarget(
  raw: string,
  _context: TerminalLinkContext,
): TerminalLinkDecision {
  const trimmed = raw.trim();
  if (!trimmed || /[\u0000-\u001f\u007f\s]/.test(trimmed)) {
    return { action: "refuse", reason: "invalid_url" };
  }
  let parsed: URL;
  try {
    parsed = new URL(trimmed);
  } catch {
    return { action: "refuse", reason: "invalid_url" };
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    return { action: "refuse", reason: "disallowed_scheme" };
  }
  if (!parsed.host) {
    return { action: "refuse", reason: "invalid_url" };
  }
  return { action: "open_local", url: trimmed };
}

export interface OpenTerminalLinkDeps {
  openUrl: (url: string) => Promise<void>;
  fallbackOpen?: (url: string) => unknown;
}

const defaultLinkDeps: OpenTerminalLinkDeps = {
  openUrl: (url) => nativeBridge.openUrl(url),
  fallbackOpen: (url) => {
    if (typeof window !== "undefined") window.open(url, "_blank", "noopener,noreferrer");
  },
};

export async function openTerminalLink(
  raw: string,
  context: TerminalLinkContext,
  deps: OpenTerminalLinkDeps = defaultLinkDeps,
): Promise<boolean> {
  const decision = resolveTerminalLinkTarget(raw, context);
  if (decision.action !== "open_local") return false;
  try {
    await deps.openUrl(decision.url);
    return true;
  } catch {
    if (!deps.fallbackOpen) return false;
    deps.fallbackOpen(decision.url);
    return true;
  }
}

function countChar(text: string, target: string): number {
  let count = 0;
  for (let i = 0; i < text.length; i += 1) {
    if (text[i] === target) count += 1;
  }
  return count;
}

/** Trim sentence punctuation and unmatched closing delimiters from a matched URL. */
export function trimMatchedUrl(raw: string): string {
  let url = raw;
  while (url.length > 0) {
    const last = url[url.length - 1];
    if (TRAILING_PUNCTUATION.has(last)) {
      url = url.slice(0, -1);
      continue;
    }
    if (last === ")" && countChar(url, ")") > countChar(url, "(")) {
      url = url.slice(0, -1);
      continue;
    }
    if (last === "]" && countChar(url, "]") > countChar(url, "[")) {
      url = url.slice(0, -1);
      continue;
    }
    if (last === "}" && countChar(url, "}") > countChar(url, "{")) {
      url = url.slice(0, -1);
      continue;
    }
    break;
  }
  return url;
}

export interface TerminalBufferLineLike {
  readonly isWrapped: boolean;
  readonly length: number;
  translateToString(trimRight?: boolean, startColumn?: number, endColumn?: number): string;
  getCell?(x: number, cell?: IBufferCell): IBufferCell | undefined;
}

export interface TerminalBufferLike {
  readonly length: number;
  getLine(y: number): TerminalBufferLineLike | undefined;
  getNullCell?(): IBufferCell;
}

interface LineSegment {
  lineIndex: number;
  text: string;
  columnOffset: number;
  joinedStart: number;
  joinedEnd: number;
}

export interface DetectedTerminalLink {
  url: string;
  range: IBufferRange;
}

function leadingSpaces(text: string): number {
  let i = 0;
  while (i < text.length && text[i] === " ") i += 1;
  return i;
}

function looksLikeHardWrappedContinuation(
  prevLineText: string,
  prevUrlPart: string,
  nextLineText: string,
  urlLineIndent: number,
  cols: number,
  alreadyHardWrapped: boolean,
): { token: string; indent: number } | null {
  const indent = leadingSpaces(nextLineText);
  if (indent > Math.max(urlLineIndent, MAX_HARD_WRAP_INDENT)) return null;
  const rest = nextLineText.slice(indent);
  if (!rest || HARD_WRAP_FORBIDDEN_START.test(rest)) return null;

  const tokenMatch = URL_TOKEN_RE.exec(rest);
  if (!tokenMatch) return null;
  const token = tokenMatch[0];
  const hasTrailingSpaces = rest.length > token.length;
  if (hasTrailingSpaces) return null;

  const prevTrimmedLen = prevLineText.length;
  const reachedWrapEdge = prevTrimmedLen >= Math.min(cols - 4, MIN_HARD_WRAP_LINE_CHARS);
  const explicitTail = HARD_WRAP_CONTINUATION_TAIL.test(prevUrlPart) && prevUrlPart.length >= MIN_HARD_WRAP_TAIL_URL_CHARS;
  if (!reachedWrapEdge && !explicitTail && !alreadyHardWrapped) return null;

  const looksLikeUrlFragment =
    alreadyHardWrapped
    || explicitTail
    || HARD_WRAP_URL_STRUCTURE.test(token)
    || /\d/.test(token)
    || (!HARD_WRAP_SENTENCE_WORD.test(token) && token.length >= 4);
  if (!looksLikeUrlFragment) return null;

  return { token, indent };
}

function buildSoftWrappedGroup(buffer: TerminalBufferLike, anchorIndex: number): LineSegment[] {
  let start = anchorIndex;
  let chars = 0;
  while (start > 0 && anchorIndex - start < MAX_JOINED_LINES && chars < MAX_JOINED_CHARS) {
    const line = buffer.getLine(start);
    if (!line?.isWrapped) break;
    const prev = buffer.getLine(start - 1);
    if (!prev) break;
    chars += prev.translateToString(true).length;
    start -= 1;
  }

  const segments: LineSegment[] = [];
  let offset = 0;
  let idx = start;
  while (idx < buffer.length && segments.length < MAX_JOINED_LINES && offset < MAX_JOINED_CHARS) {
    const line = buffer.getLine(idx);
    if (!line) break;
    if (idx > start && !line.isWrapped) break;
    const text = line.translateToString(true);
    segments.push({
      lineIndex: idx,
      text,
      columnOffset: 0,
      joinedStart: offset,
      joinedEnd: offset + text.length,
    });
    offset += text.length;
    idx += 1;
  }
  return segments;
}

function findHardWrapStartLine(buffer: TerminalBufferLike, anchorIndex: number, cols: number): number {
  let candidate = anchorIndex;
  for (let steps = 0; steps < MAX_JOINED_LINES && candidate > 0; steps += 1) {
    const currentLine = buffer.getLine(candidate);
    if (!currentLine) break;
    if (currentLine.isWrapped) {
      candidate -= 1;
      continue;
    }
    const currentText = currentLine.translateToString(true);
    const trimmed = currentText.trim();
    if (!trimmed || /\s/.test(trimmed) || HARD_WRAP_FORBIDDEN_START.test(trimmed)) break;

    const prevLine = buffer.getLine(candidate - 1);
    if (!prevLine) break;
    const prevText = prevLine.translateToString(true);
    if (prevText.length < Math.min(cols - 4, MIN_HARD_WRAP_LINE_CHARS) && !HARD_WRAP_CONTINUATION_TAIL.test(prevText)) {
      break;
    }
    candidate -= 1;
    if (/https?:\/\//i.test(prevText)) return candidate;
  }
  return anchorIndex;
}

function mapOffsetToCell(
  buffer: TerminalBufferLike,
  segments: readonly LineSegment[],
  joinedOffset: number,
  isEnd: boolean,
): { x: number; y: number } | null {
  if (segments.length === 0) return null;
  const targetOffset = isEnd ? Math.max(0, joinedOffset - 1) : joinedOffset;
  let segment = segments[segments.length - 1];
  for (const candidate of segments) {
    if (targetOffset >= candidate.joinedStart && targetOffset < candidate.joinedEnd) {
      segment = candidate;
      break;
    }
  }
  const localIndex = Math.max(0, Math.min(segment.text.length - 1, targetOffset - segment.joinedStart));
  const line = buffer.getLine(segment.lineIndex);
  if (!line) return null;

  if (!line.getCell || !buffer.getNullCell) {
    return {
      x: segment.columnOffset + localIndex + 1,
      y: segment.lineIndex + 1,
    };
  }

  const nullCell = buffer.getNullCell();
  let consumed = 0;
  for (let col = segment.columnOffset; col < line.length; col += 1) {
    const cell = line.getCell(col, nullCell);
    if (!cell) break;
    const width = cell.getWidth();
    if (width === 0) continue;
    const chars = cell.getChars();
    const charLen = chars.length || 1;
    if (consumed + charLen > localIndex) {
      return {
        x: isEnd ? col + width : col + 1,
        y: segment.lineIndex + 1,
      };
    }
    consumed += charLen;
  }
  return {
    x: segment.columnOffset + localIndex + 1,
    y: segment.lineIndex + 1,
  };
}

/**
 * Find all safe http/https links overlapping 1-based `bufferLineNumber`,
 * stitching both soft-wrapped (`isWrapped`) and hard-wrapped TUI URLs.
 */
export function findWrappedLinksAtLine(
  buffer: TerminalBufferLike,
  bufferLineNumber: number,
  cols: number,
): DetectedTerminalLink[] {
  const anchorIndex = bufferLineNumber - 1;
  if (anchorIndex < 0 || anchorIndex >= buffer.length) return [];

  const startIndex = findHardWrapStartLine(buffer, anchorIndex, cols);
  const softGroup = buildSoftWrappedGroup(buffer, startIndex);
  if (softGroup.length === 0) return [];

  const softText = softGroup.map((s) => s.text).join("");
  const results: DetectedTerminalLink[] = [];
  URL_START_RE.lastIndex = 0;

  let match: RegExpExecArray | null;
  while ((match = URL_START_RE.exec(softText)) !== null) {
    const matchStart = match.index;
    const matchEnd = matchStart + match[0].length;
    const segments: LineSegment[] = [...softGroup];
    let combined = match[0];

    // If this URL reaches the end of the soft-wrapped group, check subsequent hard-wrapped rows.
    if (matchEnd === softText.length) {
      const startSeg = segments.find((s) => matchStart >= s.joinedStart && matchStart < s.joinedEnd) ?? segments[0];
      const urlIndent = startSeg ? leadingSpaces(startSeg.text) : 0;
      let nextIdx = segments[segments.length - 1].lineIndex + 1;
      let prevLineText = segments[segments.length - 1].text;
      let totalLen = segments[segments.length - 1].joinedEnd;
      let hardWrapped = false;

      while (nextIdx < buffer.length && segments.length < MAX_JOINED_LINES && totalLen < MAX_JOINED_CHARS) {
        const nextLine = buffer.getLine(nextIdx);
        if (!nextLine) break;
        const nextText = nextLine.translateToString(true);
        if (nextLine.isWrapped) {
          const tokenMatch = URL_TOKEN_RE.exec(nextText);
          if (!tokenMatch) break;
          const token = tokenMatch[0];
          segments.push({
            lineIndex: nextIdx,
            text: token,
            columnOffset: 0,
            joinedStart: totalLen,
            joinedEnd: totalLen + token.length,
          });
          combined += token;
          totalLen += token.length;
          prevLineText = nextText;
          nextIdx += 1;
          if (token.length < nextText.length) break;
          continue;
        }

        const continuation = looksLikeHardWrappedContinuation(
          prevLineText,
          combined,
          nextText,
          urlIndent,
          cols,
          hardWrapped,
        );
        if (!continuation) break;
        segments.push({
          lineIndex: nextIdx,
          text: continuation.token,
          columnOffset: continuation.indent,
          joinedStart: totalLen,
          joinedEnd: totalLen + continuation.token.length,
        });
        combined += continuation.token;
        totalLen += continuation.token.length;
        prevLineText = nextText;
        hardWrapped = true;
        nextIdx += 1;
      }
    }

    const cleaned = trimMatchedUrl(combined);
    const safe = resolveSafeTerminalLinkUrl(cleaned);
    if (!safe) continue;

    const start = mapOffsetToCell(buffer, segments, matchStart, false);
    const end = mapOffsetToCell(buffer, segments, matchStart + cleaned.length, true);
    if (!start || !end) continue;
    if (bufferLineNumber < start.y || bufferLineNumber > end.y) continue;
    results.push({ url: safe, range: { start, end } });
  }

  return results;
}

/**
 * Unwrap a multi-line terminal selection only when the entire trimmed selection
 * is a single hard-wrapped http/https URL (such as a Claude OAuth login link).
 * Multi-line code, logs, or lists of URLs pass through unchanged.
 */
export function normalizeCopiedTerminalSelection(selection: string): string {
  if (!selection.includes("\n") && !selection.includes("\r")) return selection;
  const trimmed = selection.trim();
  if (!/^https?:\/\//i.test(trimmed)) return selection;

  const lines = trimmed.split(/\r?\n/);
  if (lines.length < 2) return selection;

  const parts: string[] = [];
  for (let i = 0; i < lines.length; i += 1) {
    const part = lines[i].trim();
    if (!part) return selection;
    if (/\s/.test(part)) return selection;
    if (i > 0 && /^https?:\/\//i.test(part)) return selection;
    parts.push(part);
  }

  const joined = parts.join("");
  return resolveSafeTerminalLinkUrl(joined) ?? selection;
}

export interface TerminalLinkProviderCallbacks {
  onActivate: (event: MouseEvent, uri: string) => void;
  onHover?: (event: MouseEvent, uri: string) => void;
  onLeave?: (event: MouseEvent, uri: string) => void;
}

export function createTerminalLinkProvider(
  term: Terminal,
  callbacks: TerminalLinkProviderCallbacks,
): ILinkProvider {
  return {
    provideLinks(bufferLineNumber: number, callback: (links: ILink[] | undefined) => void) {
      const detected = findWrappedLinksAtLine(
        term.buffer.active as unknown as TerminalBufferLike,
        bufferLineNumber,
        term.cols,
      );
      if (detected.length === 0) {
        callback(undefined);
        return;
      }
      callback(
        detected.map((item) => ({
          range: item.range,
          text: item.url,
          activate: (event, text) => callbacks.onActivate(event, text),
          hover: callbacks.onHover ? (event, text) => callbacks.onHover?.(event, text) : undefined,
          leave: callbacks.onLeave ? (event, text) => callbacks.onLeave?.(event, text) : undefined,
        })),
      );
    },
  };
}
