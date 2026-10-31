import { unwrapShellLauncher } from "./shellComposition";
import { lexShell, type LexedSegment, type ShellDialect } from "./shellLexer";

export const SHELL_FILE_LIMITS = Object.freeze({ inputBytes: 16_384, argv: 128, tokens: 256, segments: 64, operations: 8, variables: 16, path: 512, cache: 64, cacheBytes: 262_144 });

export interface ShellFileOperation {
  readonly action: "write" | "append";
  readonly path: string | null;
}

export interface ShellFileAnalysis {
  readonly operations: readonly ShellFileOperation[];
  readonly limited: boolean;
}

const EMPTY: ShellFileAnalysis = Object.freeze({ operations: Object.freeze([]), limited: false });
const LIMITED: ShellFileAnalysis = Object.freeze({ operations: Object.freeze([]), limited: true });
const CACHE = new Map<string, ShellFileAnalysis>();
let cacheBytes = 0;
const WRITERS = new Set(["set-content", "add-content", "out-file"]);
const VALUE_OPTIONS = new Set(["-value", "-encoding", "-width"]);
const SWITCH_OPTIONS = new Set(["-append", "-force", "-noclobber", "-nonewline", "-whatif", "-confirm"]);

function byteLength(value: string): number {
  if (value.length > SHELL_FILE_LIMITS.inputBytes) return value.length * 3;
  let bytes = 0;
  for (const char of value) {
    const code = char.codePointAt(0) ?? 0;
    bytes += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4;
  }
  return bytes;
}

function literal(raw: string, dialect: ShellDialect): string | null {
  let value = raw;
  if (raw.startsWith("'")) {
    if (dialect === "cmd") return null;
    if (!raw.endsWith("'")) return null;
    value = raw.slice(1, -1);
    if (dialect === "powershell") {
      if (!/^(?:[^']|'')*$/.test(value)) return null;
      value = value.replace(/''/g, "'");
    } else if (value.includes("'")) return null;
  } else if (raw.startsWith('"')) {
    if (!raw.endsWith('"')) return null;
    value = raw.slice(1, -1);
    if (/["$`]/.test(value)) return null;
    if (dialect === "posix" && value.includes("\\")) return null;
  } else if (/[\s'"$`%@,~*?\[\]{}();|&<>^]/.test(raw) || dialect === "posix" && raw.includes("\\")) return null;
  if (!value || value.length > SHELL_FILE_LIMITS.path || /[\x00-\x1f\x7f]/.test(value)) return null;
  if (dialect === "cmd" && /[%!^]/.test(value)) return null;
  return value;
}

function target(raw: string | undefined, dialect: ShellDialect, variables: Map<string, string>, wildcard: boolean): string | null {
  if (!raw) return null;
  const variable = /^\$([A-Za-z_][A-Za-z0-9_]*)$/.exec(raw);
  const path = variable ? variables.get(dialect === "powershell" ? variable[1].toLowerCase() : variable[1]) ?? null : literal(raw, dialect);
  return path && (!wildcard || !/[*?\[\]]/.test(path)) ? path : null;
}

function writeTarget(segment: LexedSegment): { raw?: string; wildcard: boolean } {
  const raw = segment.rawArgv ?? [];
  let positional: string | undefined;
  let explicit: string | undefined;
  let wildcard = true;
  for (let i = 1; i < raw.length; i++) {
    const option = raw[i].toLowerCase();
    if (option === "-path" || option === "-literalpath" || option === "-filepath") {
      if (explicit !== undefined || i + 1 >= raw.length) return { wildcard: true };
      explicit = raw[++i];
      wildcard = option === "-path";
    } else if (VALUE_OPTIONS.has(option)) {
      if (i + 1 >= raw.length || segment.argv[i + 1].startsWith("-")) return { wildcard: true };
      i++;
    } else if (SWITCH_OPTIONS.has(option)) continue;
    else if (option.startsWith("-")) return { wildcard: true };
    else if (positional === undefined) positional = raw[i];
  }
  return { raw: explicit ?? positional, wildcard };
}

function analyze(command: string, argv?: readonly string[]): ShellFileAnalysis {
  const unwrapped = unwrapShellLauncher(command, argv);
  const dialect = !unwrapped.launcher && /^\s*(?:\$[A-Za-z_]\w*\s*=|(?:Set-Content|Add-Content|Out-File)\b)/i.test(command)
    ? "powershell" : unwrapped.dialect;
  const segments = lexShell(unwrapped.command, dialect, true, SHELL_FILE_LIMITS.tokens);
  if (!segments) return EMPTY;
  if (segments.length > SHELL_FILE_LIMITS.segments) return LIMITED;
  const operations: ShellFileOperation[] = [];
  const variables = new Map<string, string>();
  let limited = false;
  let previous: LexedSegment | undefined;
  const add = (action: ShellFileOperation["action"], path: string | null) => {
    if (operations.length < SHELL_FILE_LIMITS.operations) operations.push(Object.freeze({ action, path }));
    else limited = true;
  };
  for (const segment of segments) {
    if (previous?.operator === "and" || previous?.operator === "or" || previous?.operator === "pipe") variables.clear();
    const text = unwrapped.command.slice(segment.start, segment.end);
    if (/\$\(|[{}]/.test(text)) variables.clear();
    const assignment = /^(\$?[A-Za-z_][A-Za-z0-9_]*)\s*=(?!=)\s*([\s\S]*)$/.exec(text);
    if (assignment) {
      if (/\$[A-Za-z_]\w*\s*=(?!=)/.test(assignment[2])) variables.clear();
      const name = dialect === "powershell" ? assignment[1].replace(/^\$/, "").toLowerCase() : assignment[1];
      variables.delete(name);
      const value = dialect === "powershell" && !/^["']/.test(assignment[2]) ? null : literal(assignment[2], dialect);
      if (value !== null && (dialect !== "powershell" || assignment[1].startsWith("$"))) {
        if (variables.size < SHELL_FILE_LIMITS.variables) variables.set(name, value);
        else limited = true;
      }
    } else {
      const name = segment.argv[0]?.toLowerCase();
      if (WRITERS.has(name) && segment.rawArgv?.[0].toLowerCase() === name) {
        const destination = writeTarget(segment);
        const append = name === "add-content" || name === "out-file" && segment.rawArgv?.some(arg => arg.toLowerCase() === "-append");
        add(append ? "append" : "write", target(destination.raw, dialect, variables, destination.wildcard));
      }
      if (!WRITERS.has(name) && !/^(?:get-content|echo|printf|cat)$/.test(name)) variables.clear();
    }
    for (const redirect of segment.redirectTargets ?? []) {
      if (/^(?:\d+|\*|&)?>>?$/.test(redirect.operator)) {
        const path = target(redirect.raw, dialect, variables, dialect !== "cmd");
        if (path !== "/dev/null" && path?.toLowerCase() !== "nul") add(redirect.operator.endsWith(">>") ? "append" : "write", path);
      }
    }
    previous = segment;
  }
  if (operations.length === 0) return limited ? LIMITED : EMPTY;
  return Object.freeze({ operations: Object.freeze(operations), limited });
}

export function shellFileOperations(command: string, argv?: readonly string[]): ShellFileAnalysis {
  if (argv && argv.length > SHELL_FILE_LIMITS.argv) return LIMITED;
  let bytes = byteLength(command);
  if (bytes > SHELL_FILE_LIMITS.inputBytes) return LIMITED;
  for (const arg of argv ?? []) {
    bytes += byteLength(arg);
    if (bytes > SHELL_FILE_LIMITS.inputBytes) return LIMITED;
  }
  const key = JSON.stringify([command, argv]);
  const cached = CACHE.get(key);
  if (cached) return cached;
  const result = analyze(command, argv);
  const retainedBytes = (key.length + result.operations.reduce((sum, operation) => sum + (operation.path?.length ?? 0), 0)) * 2;
  while (CACHE.size >= SHELL_FILE_LIMITS.cache || cacheBytes + retainedBytes > SHELL_FILE_LIMITS.cacheBytes) {
    const oldest = CACHE.keys().next().value as string | undefined;
    if (oldest === undefined) return result;
    const value = CACHE.get(oldest) as ShellFileAnalysis;
    cacheBytes -= (oldest.length + value.operations.reduce((sum, operation) => sum + (operation.path?.length ?? 0), 0)) * 2;
    CACHE.delete(oldest);
  }
  CACHE.set(key, result);
  cacheBytes += retainedBytes;
  return result;
}
