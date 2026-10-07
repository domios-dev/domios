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

const SESSION_HEADER_RE = /^# aider chat started at (.+)$/;
const THINKING_BLOCK_RE = /<thinking-content-[^>]*>[\s\S]*?<\/thinking-content-[^>]*>/g;

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
      if (!userText) {
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
    cursor: cursor || null,
  };
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

function readHistoryFile(filePath: string): string | null {
  try {
    const value = host.fs.readFile(filePath);
    return value === null || value === undefined ? null : String(value);
  } catch (_) {
    return null;
  }
}
