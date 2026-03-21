import type { ParsedInput } from "./types";

export class InputTracker {
  private line = "";
  private confident = true;

  /** Feed keystroke data (same data going to PTY via onData). */
  feed(data: string): void {
    let i = 0;
    while (i < data.length) {
      const ch = data[i];
      const code = ch.charCodeAt(0);

      if (ch === "\r" || ch === "\n") {
        // Enter — reset line
        this.line = "";
        this.confident = true;
      } else if (ch === "\x7f" || code === 8) {
        // Backspace — remove last char
        this.line = this.line.slice(0, -1);
      } else if (ch === "\x1b") {
        // Escape sequence — skip entirely
        i++;
        if (i < data.length && data[i] === "[") {
          // CSI sequence: ESC [ ... (letter)
          i++;
          while (i < data.length && !(/[a-zA-Z~]/).test(data[i])) i++;
          // skip the terminating letter
        } else if (i < data.length && data[i] === "O") {
          // SS3 sequence: ESC O (letter) — function keys
          i++;
        } else if (i < data.length) {
          // Other ESC + char — skip one
        }
        // Don't lose confidence on escape sequences — they come from
        // terminal focus events, PSReadLine syntax highlighting, etc.
        // Only arrow keys (history) should lose confidence, but we can't
        // reliably distinguish, so keep confident and rely on feedOutput reset.
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
      i++;
    }
  }

  /** Feed PTY output to detect prompt and re-sync. */
  feedOutput(text: string): void {
    // Strip ANSI escape sequences before checking for prompt
    const clean = text.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "")
                      .replace(/\x1b\][^\x07]*\x07/g, "")  // OSC sequences
                      .replace(/\x1b[()][0-9A-Z]/g, "")    // charset sequences
                      .replace(/[\r\x00-\x08\x0e-\x1f]/g, ""); // control chars

    // Only reset on actual new prompt — must contain newline + prompt char at end
    // This avoids resetting mid-typing when shell sends syntax highlighting
    if (!clean.includes("\n")) return;

    const lastLine = clean.split("\n").pop()?.trimEnd() || "";
    if (
      lastLine.endsWith(">") ||
      lastLine.endsWith("$") ||
      lastLine.endsWith("#") ||
      lastLine.endsWith("%") ||
      lastLine.endsWith("\u{276f}") // ❯
    ) {
      // Only reset if user hasn't typed anything recently
      // (prompt after command execution, not inline syntax highlight refresh)
      if (this.line === "" || clean.includes("\r\n") || clean.includes("\n\n")) {
        this.line = "";
        this.confident = true;
      }
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
