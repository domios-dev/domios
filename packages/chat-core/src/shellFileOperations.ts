import { unwrapShellLauncher } from "./shellComposition";
import { lexShell, type LexedSegment, type ShellDialect } from "./shellLexer";

export const SHELL_FILE_LIMITS = Object.freeze({ inputBytes: 16_384, argv: 128, tokens: 256, segments: 64, operations: 8, variables: 16, path: 512, replacements: 8, fragmentBytes: 2_048, totalFragmentBytes: 8_192, cache: 64, cacheBytes: 262_144 });

interface ReplacementFragment {
  readonly before: string;
  readonly after: string;
}

interface ReplacementChain {
  readonly path: string;
  readonly fragments: readonly ReplacementFragment[];
  readonly unsupported: boolean;
}

export interface ShellFileOperation {
  readonly action: "write" | "append";
  readonly path: string | null;
  readonly replacement?: ReplacementChain;
  readonly content?: string;
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
const QUOTED_FRAGMENT = `(?:'(?:[^']|'')*'|"[^"$\x60]*")`;
const REPLACE = new RegExp(`^\\$([A-Za-z_]\\w*)\\.Replace\\(\\s*(${QUOTED_FRAGMENT})\\s*,\\s*(${QUOTED_FRAGMENT})\\s*\\)$`, "i");
const READ = new RegExp(`^Get-Content\\s+((?:-LiteralPath|-Path)\\s+)?(${QUOTED_FRAGMENT}|\\$[A-Za-z_]\\w*|[^\\s'"$;()]+)\\s+-Raw$`, "i");

function syntaxText(source: string, segment: LexedSegment): string {
  let text = "";
  let at = segment.start;
  for (const range of segment.literalRanges ?? []) {
    text += source.slice(at, range.start) + " ".repeat(range.end - range.start);
    at = range.end;
  }
  return text + source.slice(at, segment.end);
}

function writerVariable(segment: LexedSegment): string | null {
  const args = segment.rawArgv ?? [];
  let at = 1;
  if (/^-(?:literalpath|path)$/i.test(args[at] ?? "")) at++;
  if (!args[at] || args[at].startsWith("-")) return null;
  at++;
  if (args[at]?.toLowerCase() === "-value") at++;
  return at === args.length - 1 && /^\$[A-Za-z_]\w*$/.test(args[at]) ? args[at].slice(1).toLowerCase() : null;
}

function writtenLiteral(segment: LexedSegment): string | undefined {
  const args = segment.rawArgv ?? [];
  const flag = args.findIndex((arg) => arg.toLowerCase() === "-value");
  const raw = flag >= 0 ? args[flag + 1] : args.length === 3 ? args[2] : undefined;
  if (!raw || !new RegExp(`^${QUOTED_FRAGMENT}$`).test(raw)) return undefined;
  return raw[0] === "'" ? raw.slice(1, -1).replace(/''/g, "'") : raw.slice(1, -1);
}

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
  const chains = new Map<string, ReplacementChain>();
  let replacementCount = 0;
  let fragmentBytes = 0;
  let limited = false;
  let previous: LexedSegment | undefined;
  const clear = () => { variables.clear(); chains.clear(); };
  const add = (action: ShellFileOperation["action"], path: string | null, chain?: ReplacementChain, content?: string) => {
    const replacement = chain && chain.path === path && chain.fragments.length > 0 ? chain : undefined;
    if (content !== undefined) {
      const bytes = byteLength(content);
      if (bytes > SHELL_FILE_LIMITS.fragmentBytes || fragmentBytes + bytes > SHELL_FILE_LIMITS.totalFragmentBytes) {
        limited = true;
        content = undefined;
      } else fragmentBytes += bytes;
    }
    if (operations.length < SHELL_FILE_LIMITS.operations) operations.push(Object.freeze({ action, path, ...(replacement ? { replacement } : {}), ...(content !== undefined ? { content } : {}) }));
    else limited = true;
  };
  for (const segment of segments) {
    if (previous?.operator === "and" || previous?.operator === "or" || previous?.operator === "pipe") clear();
    const text = unwrapped.command.slice(segment.start, segment.end);
    const syntax = syntaxText(unwrapped.command, segment);
    if (/\$\(|[{}]|\.Replace\([^)]*\(/i.test(syntax)) clear();
    const assignment = /^(\$?[A-Za-z_][A-Za-z0-9_]*)\s*=(?!=)\s*([\s\S]*)$/.exec(text);
    if (assignment) {
      if (/\$[A-Za-z_]\w*\s*=(?!=)/.test(syntax.slice(syntax.indexOf("=") + 1))) clear();
      const name = dialect === "powershell" ? assignment[1].replace(/^\$/, "").toLowerCase() : assignment[1];
      const chain = chains.get(name);
      variables.delete(name);
      chains.delete(name);
      const value = dialect === "powershell" && !/^["']/.test(assignment[2]) ? null : literal(assignment[2], dialect);
      if (dialect === "powershell" && assignment[1].startsWith("$")) {
        const read = READ.exec(assignment[2]);
        const path = read ? target(read[2], dialect, variables, read[1]?.trim().toLowerCase() !== "-literalpath") : null;
        const replace = REPLACE.exec(assignment[2]);
        if (path) {
          if (variables.size + chains.size < SHELL_FILE_LIMITS.variables) chains.set(name, Object.freeze({ path, fragments: Object.freeze([]), unsupported: false }));
          else limited = true;
        } else if (chain && new RegExp(`^\\$${name}\\.Replace\\([\\s,+$A-Za-z0-9_]*\\)$`, "i").test(syntax.slice(syntax.indexOf("=") + 1).trim())) {
          let next = chain;
          if (replace && replace[1].toLowerCase() === name && !chain.unsupported) {
            const decode = (raw: string) => raw[0] === "'" ? raw.slice(1, -1).replace(/''/g, "'") : raw.slice(1, -1);
            const before = decode(replace[2]);
            const after = decode(replace[3]);
            const bytes = byteLength(before) + byteLength(after);
            if (!before) next = Object.freeze({ ...chain, unsupported: true });
            else if (before === after) next = chain;
            else if (replacementCount < SHELL_FILE_LIMITS.replacements && byteLength(before) <= SHELL_FILE_LIMITS.fragmentBytes && byteLength(after) <= SHELL_FILE_LIMITS.fragmentBytes && fragmentBytes + bytes <= SHELL_FILE_LIMITS.totalFragmentBytes) {
              replacementCount++;
              fragmentBytes += bytes;
              next = Object.freeze({ ...chain, fragments: Object.freeze([...chain.fragments, Object.freeze({ before, after })]) });
            } else { limited = true; next = Object.freeze({ ...chain, unsupported: true }); }
          } else next = Object.freeze({ ...chain, unsupported: true });
          chains.set(name, next);
        } else if (!read && value === null && !/^\[Environment\]::NewLine$/i.test(assignment[2])) clear();
      }
      if (value !== null && (dialect !== "powershell" || assignment[1].startsWith("$"))) {
        if (variables.size + chains.size < SHELL_FILE_LIMITS.variables) variables.set(name, value);
        else limited = true;
      }
    } else {
      const name = segment.argv[0]?.toLowerCase();
      if (WRITERS.has(name) && segment.rawArgv?.[0].toLowerCase() === name) {
        const destination = writeTarget(segment);
        const append = name === "add-content" || name === "out-file" && segment.rawArgv?.some(arg => arg.toLowerCase() === "-append");
        const variable = writerVariable(segment);
        add(append ? "append" : "write", target(destination.raw, dialect, variables, destination.wildcard), name === "set-content" && variable && segment.redirects.length === 0 ? chains.get(variable) : undefined,
          dialect === "powershell" && name !== "out-file" ? writtenLiteral(segment) : undefined);
      }
      if (!WRITERS.has(name) && !/^(?:get-content|echo|printf|cat)$/.test(name)) clear();
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

function retainedStringBytes(key: string, result: ShellFileAnalysis): number {
  return (key.length + result.operations.reduce((sum, operation) => sum + (operation.path?.length ?? 0) + (operation.content?.length ?? 0) + (operation.replacement?.path.length ?? 0)
    + (operation.replacement?.fragments.reduce((size, fragment) => size + fragment.before.length + fragment.after.length, 0) ?? 0), 0)) * 2;
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
  const retainedBytes = retainedStringBytes(key, result);
  while (CACHE.size >= SHELL_FILE_LIMITS.cache || cacheBytes + retainedBytes > SHELL_FILE_LIMITS.cacheBytes) {
    const oldest = CACHE.keys().next().value as string | undefined;
    if (oldest === undefined) return result;
    const value = CACHE.get(oldest) as ShellFileAnalysis;
    cacheBytes -= retainedStringBytes(oldest, value);
    CACHE.delete(oldest);
  }
  CACHE.set(key, result);
  cacheBytes += retainedBytes;
  return result;
}
