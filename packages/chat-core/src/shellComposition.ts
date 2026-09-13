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

function segmentOf(lexed: LexedSegment, source: string): ShellSegment {
  const payload = inlineScript(lexed.argv)
    ?? (lexed.heredoc !== undefined ? script(heredocLanguage(lexed.argv), lexed.heredoc) : undefined);
  return {
    argv: lexed.argv,
    redirects: lexed.redirects,
    text: source.slice(lexed.start, lexed.end).trim(),
    operator: lexed.operator,
    ...(payload ? { script: payload } : {}),
  };
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
      segments: lexed.map((segment) => segmentOf(segment, unwrapped.command)),
    };
  } catch {
    return null;
  }
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
