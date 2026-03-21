import type { ParsedInput } from "./types";

export class InputTracker {
  private line = "";
  private confident = true;

  /** Feed keystroke data (same data going to PTY via onData). */
  feed(data: string): void {
    for (const ch of data) {
      const code = ch.charCodeAt(0);

      if (ch === "\r" || ch === "\n") {
        // Enter — reset line
        this.line = "";
        this.confident = true;
      } else if (ch === "\x7f" || code === 8) {
        // Backspace — remove last char
        this.line = this.line.slice(0, -1);
      } else if (ch === "\x1b") {
        // Escape sequence start — lose confidence (arrows, history, etc.)
        this.confident = false;
      } else if (ch === "\x03" || ch === "\x04") {
        // Ctrl+C / Ctrl+D — reset
        this.line = "";
        this.confident = true;
      } else if (code >= 32 && code < 127) {
        // Printable ASCII
        this.line += ch;
      } else if (code > 127) {
        // Unicode — still printable
        this.line += ch;
      }
      // Ignore other control chars
    }
  }

  /** Feed PTY output to detect prompt and re-sync. */
  feedOutput(text: string): void {
    // If output ends with a prompt-like pattern, re-sync
    const trimmed = text.trimEnd();
    if (
      trimmed.endsWith(">") ||
      trimmed.endsWith("$") ||
      trimmed.endsWith("#") ||
      trimmed.endsWith("%") ||
      trimmed.endsWith("\u{276f}") // ❯
    ) {
      this.line = "";
      this.confident = true;
    }
  }

  /** Parse current line into command + args. Returns null if not confident. */
  getCursorWord(): ParsedInput | null {
    if (!this.confident || !this.line.trim()) return null;

    const parts = this.line.trimStart().split(/\s+/);
    if (parts.length === 0) return null;

    const command = parts[0];
    const args = parts.slice(1, -1);
    const currentArg = parts.length > 1 ? parts[parts.length - 1] : "";

    // If line ends with space, currentArg is empty (user wants completions for next arg)
    if (this.line.endsWith(" ")) {
      return { command, args: parts.slice(1), currentArg: "" };
    }

    return { command, args, currentArg };
  }

  /** Check if tracker is confident about current line state. */
  isConfident(): boolean {
    return this.confident;
  }

  /** Get raw current line. */
  getLine(): string {
    return this.line;
  }

  /** Reset tracker state. */
  reset(): void {
    this.line = "";
    this.confident = true;
  }
}
