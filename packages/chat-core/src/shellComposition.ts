import { lexShell, type LexedSegment, type ShellDialect, type ShellOperator } from "./shellLexer";
import type { ToolEvent } from "./toolEvents";

export type { ShellDialect, ShellOperator };

export interface ShellScript {
  language: string;
  body: string;
  lineCount: number;
}

export interface ShellSegment {
  argv: string[];
  redirects: string[];
  /** Exact source text of the segment, without the operator joining it to the next one. */
  text: string;
  /** Operator joining this segment to the next; null on the last segment. */
  operator: ShellOperator | null;
  script?: ShellScript;
  /** Variable the segment assigns (`$response = cmd`, `out=$(cmd)`); argv is then the command it runs. */
  assignment?: string;
}

export interface AssignedCommand {
  variable: string;
  /** Short name of the command actually run, never the assignment itself. */
  label: string;
}

export interface ShellComposition {
  dialect: ShellDialect;
  launcher?: string;
  segments: ShellSegment[];
}

export interface UnwrappedShellCommand {
  command: string;
  dialect: ShellDialect;
  launcher?: string;
}

const MAX_LAUNCHER_DEPTH = 3;
const POWERSHELL_VALUE_FLAGS = new Set([
  "-executionpolicy", "-ex", "-ep", "-workingdirectory", "-wd", "-outputformat", "-of", "-inputformat", "-if",
  "-windowstyle", "-w", "-configurationname", "-settingsfile",
]);
const POSIX_SHELLS = new Set(["bash", "sh", "zsh", "dash", "ksh"]);
const INTERPRETERS: Record<string, { language: string; flags: string[] }> = {
  python: { language: "python", flags: ["-c"] },
  python3: { language: "python", flags: ["-c"] },
  python2: { language: "python", flags: ["-c"] },
  py: { language: "python", flags: ["-c"] },
  node: { language: "javascript", flags: ["-e", "--eval", "-p", "--print"] },
  bun: { language: "javascript", flags: ["-e", "--eval"] },
  ruby: { language: "ruby", flags: ["-e"] },
  perl: { language: "perl", flags: ["-e", "-E"] },
  php: { language: "php", flags: ["-r"] },
  psql: { language: "sql", flags: ["-c"] },
  sqlite3: { language: "sql", flags: [] },
};
const DIALECT_LANGUAGE: Record<ShellDialect, string> = { posix: "bash", powershell: "powershell", cmd: "batch" };

/** Lowercase executable name without directory or Windows executable suffix. */
export function commandBasename(token: string): string {
  const slash = Math.max(token.lastIndexOf("/"), token.lastIndexOf("\\"));
  return (slash >= 0 ? token.slice(slash + 1) : token).toLowerCase().replace(/\.(?:exe|cmd|bat)$/, "");
}

/** Index of the executable after leading `NAME=value` assignments and an `env` wrapper. */
export function commandArgvStart(argv: readonly string[]): number {
  let i = 0;
  while (i < argv.length && /^[A-Za-z_][A-Za-z0-9_]*=/.test(argv[i])) i++;
  if (argv[i] !== "env") return i;
  for (i++; i < argv.length; i++) {
    const token = argv[i];
    if (/^[A-Za-z_][A-Za-z0-9_]*=/.test(token)) continue;
    if ((token === "-u" || token === "--unset") && argv[i + 1] != null) {
      i++;
      continue;
    }
    if (!token.startsWith("-")) break;
  }
  return i;
}

function launcherCommand(argv: readonly string[]): UnwrappedShellCommand | null {
  const start = commandArgvStart(argv);
  const launcher = argv[start] == null ? "" : commandBasename(argv[start]);
  if (launcher === "pwsh" || launcher === "powershell") {
    for (let i = start + 1; i < argv.length; i++) {
      const flag = argv[i].toLowerCase();
      if (flag === "-command" || flag === "-c" || flag === "--command") {
        return i + 1 < argv.length ? { command: argv.slice(i + 1).join(" "), dialect: "powershell", launcher } : null;
      }
      if (!flag.startsWith("-") || flag === "-file" || flag === "-f" || flag.startsWith("-e") && !POWERSHELL_VALUE_FLAGS.has(flag)) return null;
      if (POWERSHELL_VALUE_FLAGS.has(flag)) i++;
    }
    return null;
  }
  if (POSIX_SHELLS.has(launcher)) {
    for (let i = start + 1; i < argv.length; i++) {
      const flag = argv[i];
      if (/^-[A-Za-z]*c[A-Za-z]*$/.test(flag)) {
        return i + 1 < argv.length ? { command: argv[i + 1], dialect: "posix", launcher } : null;
      }
      if (!flag.startsWith("-") && !flag.startsWith("+")) return null;
      if (flag === "-o" || flag === "+o" || flag === "-O" || flag === "+O" || flag === "--rcfile" || flag === "--init-file") i++;
    }
    return null;
  }
  if (launcher === "cmd") {
    for (let i = start + 1; i < argv.length; i++) {
      const flag = argv[i].toLowerCase();
      if (flag === "/c" || flag === "/k") {
        const command = argv.slice(i + 1).join(" ");
        const unquoted = /^"[\s\S]*"$/.test(command) ? command.slice(1, -1) : command;
        return unquoted.trim() ? { command: unquoted, dialect: "cmd", launcher } : null;
      }
      if (!flag.startsWith("/")) return null;
    }
  }
  return null;
}

function singleArgv(command: string, dialect: ShellDialect): string[] | null {
  const segments = lexShell(command, dialect);
  return segments?.length === 1 && segments[0].redirects.length === 0 && !segments[0].heredoc ? segments[0].argv : null;
}

/** Peels known shell launchers (`pwsh -Command`, `bash -lc`, `cmd /c`) off a command that is only a launcher. */
export function unwrapShellLauncher(command: string, argv?: readonly string[]): UnwrappedShellCommand {
  let unwrapped: UnwrappedShellCommand = { command, dialect: "posix" };
  let words: readonly string[] | null = argv && argv.length > 0 ? argv : singleArgv(command, "posix");
  for (let depth = 0; depth < MAX_LAUNCHER_DEPTH && words; depth++) {
    const inner = launcherCommand(words);
    if (!inner) break;
    unwrapped = { ...inner, launcher: unwrapped.launcher ?? inner.launcher };
    words = singleArgv(inner.command, inner.dialect);
  }
  return unwrapped;
}

function lineCount(body: string): number {
  return body.replace(/\r?\n$/, "").split(/\r?\n/).length;
}

function script(language: string, body: string): ShellScript {
  return { language, body, lineCount: lineCount(body) };
}

function inlineScript(argv: readonly string[]): ShellScript | undefined {
  const nested = launcherCommand(argv);
  if (nested) return script(DIALECT_LANGUAGE[nested.dialect], nested.command);
  const start = commandArgvStart(argv);
  const interpreter = argv[start] == null ? undefined : INTERPRETERS[commandBasename(argv[start])];
  if (!interpreter) return undefined;
  for (let i = start + 1; i < argv.length - 1; i++) {
    if (interpreter.flags.includes(argv[i])) return script(interpreter.language, argv[i + 1]);
    if (!argv[i].startsWith("-")) return undefined;
  }
  return undefined;
}

function heredocLanguage(argv: readonly string[]): string {
  const start = commandArgvStart(argv);
  const name = argv[start] == null ? "" : commandBasename(argv[start]);
  if (POSIX_SHELLS.has(name)) return "bash";
  if (name === "pwsh" || name === "powershell") return "powershell";
  return INTERPRETERS[name]?.language ?? "text";
}

// `$x = cmd` is never valid posix, so it reads as an assignment in either dialect; the lexer drops quotes, so match source.
const VARIABLE_ASSIGNMENT = /^(\$(?!env:)[A-Za-z_][A-Za-z0-9_]*)\s*=(?!=)\s*([\s\S]+)$/i;
const SUBSTITUTION_ASSIGNMENT = /^([A-Za-z_][A-Za-z0-9_]*)=("?)\$\(([\s\S]*)\)\2$/;

function assignedSegment(text: string, dialect: ShellDialect): { variable: string; lexed: LexedSegment } | null {
  if (dialect === "cmd") return null;
  const variable = VARIABLE_ASSIGNMENT.exec(text);
  const substitution = variable ? null : SUBSTITUTION_ASSIGNMENT.exec(text);
  let command: string;
  if (variable) {
    command = variable[2].trim();
    if (/^\([\s\S]*\)$/.test(command)) command = command.slice(1, -1).trim();
    if (/^["'@$\d[{-]/.test(command)) return null;
  } else if (substitution) {
    command = substitution[3].trim();
  } else {
    return null;
  }
  const inner = lexShell(command, dialect);
  if (!inner || inner.length !== 1 || inner[0].argv.length === 0) return null;
  return { variable: variable ? variable[1] : (substitution as RegExpExecArray)[1], lexed: inner[0] };
}

function segmentOf(lexed: LexedSegment, source: string, dialect: ShellDialect): ShellSegment {
  const text = source.slice(lexed.start, lexed.end).trim();
  const assigned = assignedSegment(text, dialect);
  const words = assigned ? assigned.lexed : lexed;
  const payload = inlineScript(words.argv)
    ?? (words.heredoc !== undefined ? script(heredocLanguage(words.argv), words.heredoc) : undefined);
  return {
    argv: words.argv,
    redirects: words.redirects,
    text,
    operator: lexed.operator,
    ...(payload ? { script: payload } : {}),
    ...(assigned ? { assignment: assigned.variable } : {}),
  };
}

function displayName(token: string): string {
  const slash = Math.max(token.lastIndexOf("/"), token.lastIndexOf("\\"));
  return (slash >= 0 ? token.slice(slash + 1) : token).replace(/\.(?:exe|cmd|bat)$/i, "");
}

/** The executable plus its first plain argument: the short name of what a segment runs. */
export function shellSegmentLabel(segment: Pick<ShellSegment, "argv" | "text">): string {
  const { argv } = segment;
  const start = commandArgvStart(argv);
  if (argv[start] == null) return segment.text;
  const name = displayName(argv[start]);
  const argument = argv[start + 1];
  if (!argument || argument.startsWith("-")) return name;
  return `${name} ${argument.length > 24 ? `${argument.slice(0, 23)}…` : argument}`;
}

/** Pure, fail-safe structure of a shell command; null whenever any part is ambiguous. */
export function parseShellComposition(command: string, argv?: readonly string[]): ShellComposition | null {
  try {
    if (typeof command !== "string" || !command.trim()) return null;
    const unwrapped = unwrapShellLauncher(command, argv);
    const lexed = lexShell(unwrapped.command, unwrapped.dialect);
    if (!lexed || lexed.length === 0) return null;
    return {
      dialect: unwrapped.dialect,
      ...(unwrapped.launcher ? { launcher: unwrapped.launcher } : {}),
      segments: lexed.map((segment) => segmentOf(segment, unwrapped.command, unwrapped.dialect)),
    };
  } catch {
    return null;
  }
}

const ASSIGNED_LIMIT = 500;
const ASSIGNED = new Map<string, AssignedCommand | null>();

/** Memoized: the variable a command's first segment assigns and the real command's label, or null. */
export function assignedCommand(command: string, argv?: readonly string[]): AssignedCommand | null {
  if (!command) return null;
  const key = argv ? JSON.stringify([argv, command]) : command;
  const cached = ASSIGNED.get(key);
  if (cached !== undefined) return cached;
  const segments = parseShellComposition(command, argv)?.segments ?? [];
  const first = segments[0];
  const value = first?.assignment
    ? { variable: first.assignment, label: shellSegmentLabel(first) + (segments.length > 1 ? ` +${segments.length - 1}` : "") }
    : null;
  if (ASSIGNED.size >= ASSIGNED_LIMIT) ASSIGNED.delete(ASSIGNED.keys().next().value as string);
  ASSIGNED.set(key, value);
  return value;
}

function inputCommand(input: Record<string, unknown> | undefined): string {
  for (const key of ["command", "cmd"]) {
    const value = input?.[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return "";
}

/** The command a tool event ran, plus its argv when the source supplied one. */
export function toolCommand(event: Pick<ToolEvent, "detail" | "summary" | "result" | "message">): { command: string; argv?: string[] } {
  // Detail is presentation text and may carry extra lines (a cwd note); the source's own command field is exact.
  const command = inputCommand(event.message.toolInput) || event.detail?.trim() || event.summary || event.result?.trim() || "";
  const argv = event.message.toolInput?.argv;
  return Array.isArray(argv) && argv.length > 0 && argv.every((value) => typeof value === "string")
    ? { command, argv: argv as string[] }
    : { command };
}
