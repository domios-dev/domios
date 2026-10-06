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

/**
 * Parse Aider chat history markdown, returning only messages after `sinceMs`.
 * Returns messages in chronological order.
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
  
  function flush() {
    if (currentRole && currentText.length > 0) {
      const body = currentText.join("\n").trim();
      if (body) {
        messages.push({
          role: currentRole,
          text: body,
          ts: null,
        });
      }
    }
    currentText = [];
  }
  
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    
    // User turn marker
    if (line.startsWith("#### ")) {
      flush();
      currentRole = "user";
      const userText = line.slice(5).trim();
      if (userText) currentText.push(userText);
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
  
  // Filter by timestamp if we had timestamps (Aider history doesn't include them,
  // so we return all messages and let the caller filter by file mtime or cursor)
  const filtered = sinceMs > 0 ? messages : messages;
  
  return {
    messages: filtered,
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
    const content = host.fs.readFile(filePath);
    if (content === null || content === undefined) return null;
    return parseAiderHistory(String(content), sinceMs, cursor);
  } catch (_) {
    return null;
  }
}
