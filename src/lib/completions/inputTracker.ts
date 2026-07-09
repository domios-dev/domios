import type { ParsedInput } from "./types";

export class InputTracker {
  private line = "";
  private cursor = 0;
  private confident = true;

  /** Feed keystroke data (same data going to PTY via onData). */
  feed(data: string): void {
    let i = 0;
    while (i < data.length) {
      const ch = data[i];
      const code = ch.charCodeAt(0);

      if (ch === "\r" || ch === "\n") {
        this.line = "";
        this.cursor = 0;
        this.confident = true;
      } else if (ch === "\x7f" || code === 8) {
        this.backspace();
      } else if (ch === "\x1b") {
        i = this.feedEscape(data, i);
      } else if (ch === "\x03" || ch === "\x04") {
        this.line = "";
        this.cursor = 0;
        this.confident = true;
      } else if (ch === "\x01") {
        this.cursor = 0;
      } else if (ch === "\x05") {
        this.cursor = this.line.length;
      } else if (ch === "\x0b") {
        this.line = this.line.slice(0, this.cursor);
      } else if (ch === "\x15") {
        this.line = this.line.slice(this.cursor);
        this.cursor = 0;
      } else if (ch === "\x17") {
        this.deleteWordBeforeCursor();
      } else if (code >= 32 && code < 127) {
        this.insert(ch);
      } else if (code > 127) {
        this.insert(ch);
      }
      i++;
    }
  }

  /** Feed PTY output to detect prompt and re-sync. */
  feedOutput(text: string): void {
    const clean = text.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "")
                      .replace(/\x1b\][^\x07]*\x07/g, "")  // OSC sequences
                      .replace(/\x1b[()][0-9A-Z]/g, "")    // charset sequences
                      .replace(/[\r\x00-\x08\x0e-\x1f]/g, ""); // control chars

    if (!clean.includes("\n")) return;

    const lastLine = clean.split("\n").pop()?.trimEnd() || "";
    if (
      lastLine.endsWith(">") ||
      lastLine.endsWith("$") ||
      lastLine.endsWith("#") ||
      lastLine.endsWith("%") ||
      lastLine.endsWith("\u{276f}") // ❯
    ) {
      // Only reset if user hasn't typed anything recently (prompt after command execution, not inline syntax highlight refresh)
      if (this.line === "" || clean.includes("\r\n") || clean.includes("\n\n")) {
        this.line = "";
        this.cursor = 0;
        this.confident = true;
      }
    }
  }

  /** Parse current line into command + args. Returns null if not confident. */
  getCursorWord(): ParsedInput | null {
    if (!this.confident || !this.line.trim()) return null;

    const leading = this.line.match(/^\s*/)?.[0].length ?? 0;
    const commandEnd = findTokenEnd(this.line, leading);
    const command = this.line.slice(leading, commandEnd);
    if (!command) return null;

    const tokens = tokenize(this.line);
    const argsBeforeCursor = tokens
      .filter((token) => token.start >= commandEnd && token.end < this.cursor)
      .map((token) => token.text);

    const current = tokenAtCursor(this.line, this.cursor, commandEnd);
    if (!current) {
      return {
        command,
        args: argsBeforeCursor,
        currentArg: "",
        cursor: this.cursor,
        currentArgStart: this.cursor,
        currentArgEnd: this.cursor,
      };
    }

    const args = tokens
      .filter((token) => token.start >= commandEnd && token.end < current.start)
      .map((token) => token.text);
    return {
      command,
      args,
      currentArg: current.text,
      cursor: this.cursor,
      currentArgStart: current.start,
      currentArgEnd: current.end,
    };
  }

  /** Check if tracker is confident about current line state. */
  isConfident(): boolean {
    return this.confident;
  }

  /** Get raw current line. */
  getLine(): string {
    return this.line;
  }

  getCursor(): number {
    return this.cursor;
  }

  /** Reset tracker state. */
  reset(): void {
    this.line = "";
    this.cursor = 0;
    this.confident = true;
  }

  private insert(text: string): void {
    this.line = this.line.slice(0, this.cursor) + text + this.line.slice(this.cursor);
    this.cursor += text.length;
  }

  private backspace(): void {
    if (this.cursor === 0) return;
    this.line = this.line.slice(0, this.cursor - 1) + this.line.slice(this.cursor);
    this.cursor -= 1;
  }

  private delete(): void {
    if (this.cursor >= this.line.length) return;
    this.line = this.line.slice(0, this.cursor) + this.line.slice(this.cursor + 1);
  }

  private deleteWordBeforeCursor(): void {
    if (this.cursor === 0) return;
    let start = this.cursor;
    while (start > 0 && /\s/.test(this.line[start - 1])) start--;
    while (start > 0 && !/\s/.test(this.line[start - 1])) start--;
    this.line = this.line.slice(0, start) + this.line.slice(this.cursor);
    this.cursor = start;
  }

  private feedEscape(data: string, start: number): number {
    const next = data[start + 1];
    if (next === "[") return this.feedCsi(data, start + 2);
    if (next === "O") return Math.min(start + 2, data.length - 1);
    this.confident = false;
    return Math.min(start + 1, data.length - 1);
  }

  private feedCsi(data: string, start: number): number {
    let i = start;
    while (i < data.length && !(/[A-Za-z~]/).test(data[i])) i++;
    if (i >= data.length) {
      this.confident = false;
      return data.length - 1;
    }

    const params = data.slice(start, i);
    const final = data[i];
    if (final === "D") this.cursor = Math.max(0, this.cursor - repeatCount(params));
    else if (final === "C") this.cursor = Math.min(this.line.length, this.cursor + repeatCount(params));
    else if (final === "H") this.cursor = 0;
    else if (final === "F") this.cursor = this.line.length;
    else if (final === "~") this.feedTildeCsi(params);
    else this.confident = false;
    return i;
  }

  private feedTildeCsi(params: string): void {
    const code = params.split(";")[0];
    if (code === "1" || code === "7") this.cursor = 0;
    else if (code === "4" || code === "8") this.cursor = this.line.length;
    else if (code === "3") this.delete();
    else this.confident = false;
  }
}

interface Token {
  text: string;
  start: number;
  end: number;
}

function repeatCount(params: string): number {
  const raw = Number.parseInt(params.split(";")[0] || "1", 10);
  return Number.isFinite(raw) && raw > 0 ? raw : 1;
}

function findTokenEnd(line: string, start: number): number {
  const token = readToken(line, start);
  return token?.end ?? start;
}

function tokenAtCursor(line: string, cursor: number, minimumStart: number): Token | null {
  for (const token of tokenize(line)) {
    if (token.end < minimumStart) continue;
    if (cursor >= token.start && cursor <= token.end) {
      return { ...token, text: line.slice(token.start, cursor) };
    }
  }
  return null;
}

function tokenize(line: string): Token[] {
  const tokens: Token[] = [];
  let i = 0;
  while (i < line.length) {
    while (i < line.length && /\s/.test(line[i])) i++;
    const token = readToken(line, i);
    if (!token) break;
    tokens.push(token);
    i = token.end;
  }
  return tokens;
}

function readToken(line: string, start: number): Token | null {
  if (start >= line.length) return null;
  let quote: string | null = null;
  let escaped = false;
  let i = start;
  while (i < line.length) {
    const ch = line[i];
    if (escaped) {
      escaped = false;
    } else if (ch === "\\") {
      escaped = true;
    } else if (quote) {
      if (ch === quote) quote = null;
    } else if (ch === "'" || ch === "\"") {
      quote = ch;
    } else if (/\s/.test(ch)) {
      break;
    }
    i++;
  }
  return { text: line.slice(start, i), start, end: i };
}
