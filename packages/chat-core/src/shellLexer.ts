export type ShellDialect = "posix" | "powershell" | "cmd";
export type ShellOperator = "pipe" | "then" | "and" | "or";

export interface LexedSegment {
  argv: string[];
  redirects: string[];
  heredoc?: string;
  start: number;
  end: number;
  operator: ShellOperator | null;
}

class UnsupportedShell extends Error {}

const POSIX_ESCAPABLE = new Set([..." \t\"'\\$`|&;<>(){}#*?[]~!"]);
const POSIX_DOUBLE_QUOTE_ESCAPABLE = new Set(["\"", "\\", "$", "`"]);
const COMPOUND_KEYWORDS = new Set([
  "if", "elif", "else", "fi", "then", "for", "foreach", "while", "until", "do", "done",
  "case", "esac", "function", "select", "switch", "try", "catch", "finally", "{", "}",
]);

interface PendingHeredoc { delimiter: string; stripTabs: boolean; segment: LexedSegment }

function unsupported(): never {
  throw new UnsupportedShell();
}

function draft(): LexedSegment {
  return { argv: [], redirects: [], start: -1, end: -1, operator: null };
}

function isStrong(operator: ShellOperator | null | undefined): boolean {
  return operator === "pipe" || operator === "and" || operator === "or";
}

class Lexer {
  private i = 0;
  private word = "";
  private inWord = false;
  private target: { kind: "redirect"; op: string } | { kind: "heredoc"; stripTabs: boolean } | null = null;
  private current = draft();
  private readonly segments: LexedSegment[] = [];
  private readonly heredocs: PendingHeredoc[] = [];

  constructor(private readonly src: string, private readonly dialect: ShellDialect) {}

  run(): LexedSegment[] {
    while (this.i < this.src.length) this.step();
    this.close(null, false);
    if (this.heredocs.length > 0) unsupported();
    return this.segments;
  }

  private get escape(): string {
    return this.dialect === "posix" ? "\\" : this.dialect === "powershell" ? "`" : "^";
  }

  private touch(from: number, to: number): void {
    if (this.current.start < 0) this.current.start = from;
    this.current.end = Math.max(this.current.end, to);
  }

  private append(text: string, from: number, to: number): void {
    this.word += text;
    this.inWord = true;
    this.touch(from, to);
  }

  private endWord(): void {
    if (!this.inWord) return;
    const value = this.word;
    this.word = "";
    this.inWord = false;
    const target = this.target;
    this.target = null;
    if (!target) this.current.argv.push(value);
    else if (target.kind === "redirect") this.current.redirects.push(target.op + value);
    else this.heredocs.push({ delimiter: value, stripTabs: target.stripTabs, segment: this.current });
  }

  private close(operator: ShellOperator | null, soft: boolean): void {
    this.endWord();
    if (this.target) unsupported();
    const segment = this.current;
    const previous = this.segments[this.segments.length - 1];
    if (segment.argv.length === 0) {
      if (segment.redirects.length > 0) unsupported();
      if (operator === null) {
        if (isStrong(previous?.operator)) unsupported();
        if (previous) previous.operator = null;
        return;
      }
      if (operator === "then" && (soft || !isStrong(previous?.operator))) return;
      unsupported();
    }
    if (COMPOUND_KEYWORDS.has(segment.argv[0].toLowerCase())) unsupported();
    segment.operator = operator;
    this.segments.push(segment);
    this.current = draft();
  }

  private step(): void {
    const { src, dialect } = this;
    const ch = src[this.i];
    const next = src[this.i + 1];

    if (ch === this.escape && (next === "\n" || (next === "\r" && src[this.i + 2] === "\n"))) {
      this.i += next === "\n" ? 2 : 3;
      return;
    }
    if (ch === " " || ch === "\t" || (ch === "\r" && next !== "\n")) {
      this.endWord();
      this.i++;
      return;
    }
    if (ch === "\r" || ch === "\n") {
      const newline = ch === "\r" ? this.i + 1 : this.i;
      this.endWord();
      const resume = this.heredocs.length > 0 ? this.readHeredocs(newline + 1) : newline + 1;
      this.close("then", true);
      this.i = resume;
      return;
    }
    if (ch === "#" && !this.inWord && dialect !== "cmd") {
      while (this.i < src.length && src[this.i] !== "\n" && src[this.i] !== "\r") this.i++;
      return;
    }
    if (ch === ";" && dialect !== "cmd") {
      this.close("then", false);
      this.i++;
      return;
    }
    if (ch === "|") {
      this.close(next === "|" ? "or" : "pipe", false);
      this.i += next === "|" ? 2 : 1;
      return;
    }
    if (ch === "&") {
      if (next === "&") {
        this.close("and", false);
        this.i += 2;
      } else if (next === ">" && dialect !== "cmd") {
        this.endWord();
        this.redirect("");
      } else if (dialect === "cmd") {
        this.close("then", false);
        this.i++;
      } else if (!this.inWord && this.current.argv.length === 0 && !this.target) {
        this.i++;
      } else {
        unsupported();
      }
      return;
    }
    if (ch === ">" || ch === "<") {
      this.angle(ch, next);
      return;
    }
    if (ch === "'") {
      if (dialect === "cmd") this.literal(ch);
      else this.singleQuoted();
      return;
    }
    if (ch === "\"") {
      this.doubleQuoted();
      return;
    }
    if (ch === "`" && dialect !== "cmd") {
      if (dialect === "powershell") this.escaped(1);
      else this.backtickSubstitution(this.i);
      return;
    }
    if ((ch === "$" || ch === "@") && dialect !== "cmd") {
      if (next === "(" || next === "{") {
        const end = this.balanced(this.i + 1);
        this.append(src.slice(this.i, end), this.i, end);
        this.i = end;
        return;
      }
      if (ch === "@" && (next === "'" || next === "\"") && this.hereStringStart(this.i + 2) >= 0) {
        this.hereString(next);
        return;
      }
      if (ch === "$" && next === "'" && dialect === "posix") {
        this.i++;
        this.singleQuoted(true);
        return;
      }
    }
    if (ch === "(" || ch === ")" || ch === "{" || ch === "}") {
      if (dialect === "powershell" && (ch === "(" || ch === "{")) {
        const end = this.balanced(this.i);
        this.append(src.slice(this.i, end), this.i, end);
        this.i = end;
        return;
      }
      if (dialect !== "cmd" && (ch === "(" || ch === ")")) unsupported();
      if (dialect === "powershell") unsupported();
      this.literal(ch);
      return;
    }
    if (ch === "\\" && dialect === "posix" && next !== undefined) {
      if (POSIX_ESCAPABLE.has(next)) this.escaped(1);
      else this.literal(ch);
      return;
    }
    if (ch === "^" && dialect === "cmd" && next !== undefined) {
      this.escaped(1);
      return;
    }
    this.literal(ch);
  }

  private literal(ch: string): void {
    this.append(ch, this.i, this.i + 1);
    this.i++;
  }

  private escaped(width: number): void {
    const at = this.i;
    const value = this.src[at + width];
    if (value === undefined) unsupported();
    this.append(value, at, at + width + 1);
    this.i = at + width + 1;
  }

  private angle(ch: string, next: string | undefined): void {
    const { src, dialect } = this;
    if (ch === "<" && next === "<") {
      if (dialect !== "posix") unsupported();
      this.endWord();
      if (src[this.i + 2] === "<") {
        this.redirect("");
        return;
      }
      const stripTabs = src[this.i + 2] === "-";
      this.i += stripTabs ? 3 : 2;
      this.target = { kind: "heredoc", stripTabs };
      this.skipBlanks();
      return;
    }
    if (ch === "<" && (next === "(" || next === "#")) unsupported();
    if (ch === ">" && next === "(") unsupported();
    const prefix = this.inWord && /^(?:\d+|\*)$/.test(this.word) ? this.word : "";
    if (prefix) {
      this.word = "";
      this.inWord = false;
    } else {
      this.endWord();
    }
    this.redirect(prefix);
  }

  private redirect(prefix: string): void {
    if (this.target) unsupported();
    const { src } = this;
    const from = this.i;
    let op = prefix;
    while (src[this.i] === "&" || src[this.i] === ">" || src[this.i] === "<") {
      op += src[this.i];
      this.i++;
      if (op.length > prefix.length + 3) unsupported();
    }
    this.touch(from, this.i);
    const fd = /^(?:\d+|-)/.exec(src.slice(this.i));
    if (op.endsWith("&") && fd) {
      this.i += fd[0].length;
      this.current.redirects.push(op + fd[0]);
      this.touch(from, this.i);
      return;
    }
    this.target = { kind: "redirect", op };
    this.skipBlanks();
  }

  private skipBlanks(): void {
    while (this.src[this.i] === " " || this.src[this.i] === "\t") this.i++;
    const ch = this.src[this.i];
    if (ch === undefined || ch === "\n" || ch === "\r" || ch === "|" || ch === ";" || ch === "&") unsupported();
  }

  private singleQuoted(ansi = false): void {
    const { src, dialect } = this;
    const open = this.i;
    let value = "";
    for (let i = open + 1; i < src.length; i++) {
      const ch = src[i];
      if (ansi && ch === "\\" && i + 1 < src.length) {
        value += src[++i];
      } else if (ch === "'") {
        if (dialect === "powershell" && src[i + 1] === "'") {
          value += "'";
          i++;
          continue;
        }
        this.append(value, open, i + 1);
        this.i = i + 1;
        return;
      } else {
        value += ch;
      }
    }
    unsupported();
  }

  private doubleQuoted(): void {
    const { src, dialect } = this;
    const open = this.i;
    let value = "";
    for (let i = open + 1; i < src.length; i++) {
      const ch = src[i];
      const next = src[i + 1];
      if (ch === "\"") {
        if (dialect === "powershell" && next === "\"") {
          value += "\"";
          i++;
          continue;
        }
        this.append(value, open, i + 1);
        this.i = i + 1;
        return;
      }
      if (dialect === "posix" && ch === "\\" && next !== undefined) {
        if (next === "\n") i++;
        else if (POSIX_DOUBLE_QUOTE_ESCAPABLE.has(next)) value += src[++i];
        else value += ch;
      } else if (dialect === "powershell" && ch === "`" && next !== undefined) {
        i++;
        value += next === "n" ? "\n" : next === "t" ? "\t" : next === "r" ? "\r" : next === "0" ? "\0" : next;
      } else if (dialect !== "cmd" && ch === "$" && next === "(") {
        const end = this.balanced(i + 1);
        value += src.slice(i, end);
        i = end - 1;
      } else if (dialect === "posix" && ch === "`") {
        const end = this.backtickEnd(i);
        value += src.slice(i, end);
        i = end - 1;
      } else {
        value += ch;
      }
    }
    unsupported();
  }

  private backtickEnd(open: number): number {
    for (let i = open + 1; i < this.src.length; i++) {
      if (this.src[i] === "\\") i++;
      else if (this.src[i] === "`") return i + 1;
    }
    return unsupported();
  }

  private backtickSubstitution(open: number): void {
    const end = this.backtickEnd(open);
    this.append(this.src.slice(open, end), open, end);
    this.i = end;
  }

  /** Index after the pair closing the opener at `open`, skipping quoted text and nested pairs. */
  private balanced(open: number): number {
    const { src, dialect } = this;
    const closers: string[] = [];
    for (let i = open; i < src.length; i++) {
      const ch = src[i];
      if (ch === "(") closers.push(")");
      else if (ch === "{") closers.push("}");
      else if (ch === ")" || ch === "}") {
        if (closers.pop() !== ch) unsupported();
        if (closers.length === 0) return i + 1;
      } else if (ch === "'" || ch === "\"") {
        const quote = ch;
        for (i++; i < src.length && src[i] !== quote; i++) {
          if (quote === "\"" && (src[i] === "\\" || src[i] === "`")) i++;
        }
        if (i >= src.length) unsupported();
      } else if (ch === this.escape && dialect !== "cmd") {
        i++;
      }
    }
    return unsupported();
  }

  /** Index where a here-string body starts, when the opener is followed only by a line break. */
  private hereStringStart(at: number): number {
    if (this.src[at] === "\n") return at + 1;
    if (this.src[at] === "\r" && this.src[at + 1] === "\n") return at + 2;
    return -1;
  }

  private hereString(quote: string): void {
    const { src } = this;
    const open = this.i;
    const bodyStart = this.hereStringStart(open + 2);
    const closer = new RegExp(`\\r?\\n${quote === "'" ? "'" : "\""}@`, "g");
    closer.lastIndex = bodyStart - 1;
    const match = closer.exec(src);
    if (!match) unsupported();
    const end = match.index + match[0].length;
    this.append(src.slice(bodyStart, match.index), open, end);
    this.i = end;
  }

  /** Consumes every pending heredoc body starting at `from`; returns the index after the last delimiter line. */
  private readHeredocs(from: number): number {
    const { src } = this;
    let position = from;
    for (const heredoc of this.heredocs.splice(0)) {
      const lines: string[] = [];
      for (;;) {
        if (position > src.length) unsupported();
        const newline = src.indexOf("\n", position);
        const lineEnd = newline < 0 ? src.length : newline;
        const line = src.slice(position, lineEnd).replace(/\r$/, "");
        position = lineEnd + 1;
        const candidate = heredoc.stripTabs ? line.replace(/^\t+/, "") : line;
        if (candidate === heredoc.delimiter) break;
        lines.push(candidate);
        if (newline < 0) unsupported();
      }
      heredoc.segment.heredoc = lines.join("\n");
    }
    return Math.min(position, src.length);
  }
}

/** Splits a shell command into operator-joined segments; null for anything ambiguous or unsupported. */
export function lexShell(src: string, dialect: ShellDialect): LexedSegment[] | null {
  if (typeof src !== "string") return null;
  try {
    return new Lexer(src, dialect).run();
  } catch {
    return null;
  }
}
