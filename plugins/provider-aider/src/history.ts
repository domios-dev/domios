/**
 * Parser for Aider's `.aider.chat.history.md` transcript.
 * 
 * Format:
 * - User turns start with `#### ` followed by the user message
 * - Assistant text follows until the next `#### ` or end of file
 * - Content before `launchTimeMs` is excluded
 */

export interface HistoryMessage {
  role: "user" | "assistant";
  text: string;
  ts: number | null;
}

export interface HistoryParseResult {
  messages: HistoryMessage[];
  cursor: string | null;
}

export interface HistoryBlock {
  /** Stable id: the header's own timestamp string. */
  id: string;
  startMs: number;
  content: string;
}

const SESSION_HEADER_RE = /^# aider chat started at (.+)$/;
const THINKING_BLOCK_RE = /<thinking-content-[^>]*>[\s\S]*?<\/thinking-content-[^>]*>/g;
const META_LINE_RE = /^> /;

/**
 * Aider writes the session header in LOCAL wall-clock time with no zone
 * suffix. `Date.parse` would read that as UTC and shift the block by the
 * machine's offset, so the header is parsed as a local timestamp and the
 * caller compares it against a `sinceMs` derived the same way.
 */
function parseSessionStartMs(line: string): number | null {
  const match = line.match(SESSION_HEADER_RE);
  if (!match) return null;
  const stamp = match[1].trim();
  const parts = stamp.match(
    /^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?$/,
  );
  if (!parts) {
    const ms = Date.parse(stamp);
    return Number.isNaN(ms) ? null : ms;
  }
  const [, y, mo, d, h, mi, s, frac] = parts;
  const millis = frac ? Number(`0.${frac}`) * 1000 : 0;
  return new Date(
    Number(y),
    Number(mo) - 1,
    Number(d),
    Number(h),
    Number(mi),
    Number(s),
    Math.round(millis),
  ).getTime();
}

/**
 * Parse Aider chat history markdown, returning only messages from the session
 * block that began at or after `sinceMs` (this tab's launch). Older blocks are
 * never returned. Messages are in chronological order.
 */
export function parseAiderHistory(
  content: string,
  sinceMs: number,
  cursor?: string | null,
): HistoryParseResult {
  const text = String(content || "");
  const lines = text.split(/\r?\n/);
  const messages: HistoryMessage[] = [];

  let currentRole: "user" | "assistant" | null = null;
  let currentText: string[] = [];
  let blockStartMs: number | null = null;
  let blockAccepted = sinceMs <= 0;

  function flush() {
    if (blockAccepted && currentRole && currentText.length > 0) {
      let body = currentText.join("\n").trim();
      if (currentRole === "assistant") {
        body = body.replace(THINKING_BLOCK_RE, "").trim();
        body = body
          .split(/\r?\n/)
          .filter((line) => !META_LINE_RE.test(line))
          .join("\n")
          .trim();
      }
      if (body) {
        messages.push({
          role: currentRole,
          text: body,
          ts: blockStartMs,
        });
      }
    }
    currentText = [];
  }

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    const sessionStartMs = parseSessionStartMs(line);
    if (sessionStartMs !== null) {
      flush();
      currentRole = null;
      blockStartMs = sessionStartMs;
      blockAccepted = sessionStartMs >= sinceMs;
      continue;
    }

    if (!blockAccepted) continue;

    // User turn marker
    if (line.startsWith("#### ")) {
      flush();
      const userText = line.slice(5).trim();
      // A blank `#### ` line is not a user turn; skip it entirely so it
      // never becomes an empty user row in Chat.
      if (!userText || userText === "<blank>") {
        currentRole = null;
        continue;
      }
      currentRole = "user";
      currentText.push(userText);
      continue;
    }

    // Assistant text (or continuation)
    if (currentRole === null) {
      // Content before any user turn - skip
      continue;
    }

    // If we were in a user turn and hit non-empty content, switch to assistant
    if (currentRole === "user" && line.trim()) {
      flush();
      currentRole = "assistant";
    }

    currentText.push(line);
  }

  flush();

  return {
    messages,
    cursor: String(messages.length),
  };
}

/**
 * Split history content into session blocks. Each block starts at a
 * `# aider chat started at <ts>` header; content before the first header is
 * dropped. The block id is the header's own timestamp string — stable across
 * reads of the same file.
 */
export function listAiderHistoryBlocks(content: string): HistoryBlock[] {
  const text = String(content || "");
  const lines = text.split(/\r?\n/);
  const blocks: HistoryBlock[] = [];
  let current: { id: string; startMs: number; lines: string[] } | null = null;

  function flush() {
    if (!current) return;
    blocks.push({
      id: current.id,
      startMs: current.startMs,
      content: current.lines.join("\n"),
    });
    current = null;
  }

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const sessionStartMs = parseSessionStartMs(line);
    if (sessionStartMs !== null) {
      flush();
      const match = line.match(SESSION_HEADER_RE);
      current = {
        id: match ? match[1].trim() : String(sessionStartMs),
        startMs: sessionStartMs,
        lines: [],
      };
      continue;
    }
    if (current) current.lines.push(line);
  }
  flush();
  return blocks;
}

/**
 * Read and parse Aider history from a file path.
 * Returns null if the file cannot be read.
 */
export function readAiderHistory(
  filePath: string,
  sinceMs: number,
  cursor?: string | null,
): HistoryParseResult | null {
  try {
    const content = readHistoryFile(filePath);
    if (content === null || content === undefined) return null;
    return parseAiderHistory(String(content), sinceMs, cursor);
  } catch (_) {
    return null;
  }
}

/**
 * Parse appended markdown history into user/assistant rows for the file-tail
 * chat route. `chunk` is the raw appended text (possibly a partial tail).
 * Blank `#### ` turns are skipped; thinking-content blocks are stripped from
 * assistant text. `recordIndex` is the chunk line each row was parsed from.
 *
 * Rows are RichMessage-shaped (`{role, blocks, ts}`) so the core's
 * `rows_from_plugin` can deserialize them; one malformed row would drop the
 * whole window, so this never throws and emits only what the chunk contains.
 */
export function parseAiderHistoryDelta(chunk: string): {
  messages: Array<{
    role: "user" | "assistant";
    blocks: Array<{ kind: "text"; data: { text: string } }>;
    ts: number | null;
    recordIndex: number;
  }>;
} {
  const text = String(chunk || "");
  const lines = text.split(/\r?\n/);
  const messages: Array<{
    role: "user" | "assistant";
    blocks: Array<{ kind: "text"; data: { text: string } }>;
    ts: number | null;
    recordIndex: number;
  }> = [];
  let currentRole: "user" | "assistant" | null = null;
  let currentText: string[] = [];
  let currentStart = 0;

  function flush() {
    if (currentRole && currentText.length > 0) {
      let body = currentText.join("\n").trim();
      if (currentRole === "assistant") {
        body = body.replace(THINKING_BLOCK_RE, "").trim();
        body = body
          .split(/\r?\n/)
          .filter((line) => !META_LINE_RE.test(line))
          .join("\n")
          .trim();
      }
      if (body) {
        messages.push({
          role: currentRole,
          blocks: [{ kind: "text", data: { text: body } }],
          ts: null,
          recordIndex: currentStart,
        });
      }
    }
    currentText = [];
  }

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (line.startsWith("#### ")) {
      flush();
      const userText = line.slice(5).trim();
      if (!userText || userText === "<blank>") {
        currentRole = null;
        continue;
      }
      currentRole = "user";
      currentStart = i;
      currentText.push(userText);
      continue;
    }
    if (currentRole === null) continue;
    if (currentRole === "user" && line.trim()) {
      flush();
      currentRole = "assistant";
      currentStart = i;
    }
    currentText.push(line);
  }
  flush();
  return { messages };
}

export { readHistoryFile };

function readHistoryFile(filePath: string): string | null {
  const globalHost = (globalThis as { host?: { fs?: Record<string, unknown> } }).host;
  const fs = globalHost?.fs;
  if (fs && typeof fs.readFile === "function") {
    try {
      const value = (fs.readFile as (p: string) => unknown)(filePath);
      return value === null || value === undefined ? null : String(value);
    } catch (_) {
      return null;
    }
  }
  return null;
}
