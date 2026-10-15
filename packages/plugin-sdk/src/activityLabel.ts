const MAX_LABEL_CHARS = 60;
const POWERSHELL = new Set(["pwsh", "powershell"]);
const POSIX_SHELLS = new Set(["bash", "sh", "zsh"]);

interface Parsed { tokens: string[]; rest: (index: number) => string }

function executableName(token: string): string {
  const leaf = token.split(/[\\/]+/).filter(Boolean).pop() ?? token;
  return leaf.replace(/\.(exe|cmd|bat|com)$/i, "");
}

function unquoteWhole(value: string): string {
  const trimmed = value.trim();
  if (trimmed.length < 2 || !['"', "'"].includes(trimmed[0]) || trimmed[0] !== trimmed[trimmed.length - 1]) return trimmed;
  if (!/^"(?:[^"\\]|\\.)*"$/.test(trimmed) && !/^'[^']*'$/.test(trimmed)) return trimmed;
  try {
    const parsed: unknown = JSON.parse(trimmed);
    return typeof parsed === "string" ? parsed : trimmed;
  } catch {
    return trimmed.slice(1, -1);
  }
}

function tokenize(source: string): Parsed {
  const tokens: string[] = [];
  const starts: number[] = [];
  let current = "";
  let open = false;
  let quote = "";
  for (let i = 0; i < source.length; i++) {
    const char = source[i];
    if (quote) {
      if (char === "\\" && quote === '"' && source[i + 1] === '"') current += source[++i];
      else if (char === quote) quote = "";
      else current += char;
    } else if (char === '"' || char === "'") {
      if (!open) { starts.push(i); open = true; }
      quote = char;
    } else if (/\s/.test(char)) {
      if (open) { tokens.push(current); current = ""; open = false; }
    } else {
      if (!open) { starts.push(i); open = true; }
      current += char;
    }
  }
  if (open) tokens.push(current);
  return { tokens, rest: (index) => (index < starts.length ? source.slice(starts[index]) : "") };
}

function argvParsed(argv: string[]): Parsed {
  const display = argv.map((argument) => (argument === "" || /[\s"']/.test(argument) ? JSON.stringify(argument) : argument));
  return { tokens: argv, rest: (index) => display.slice(index).join(" ") };
}

function isCommandSwitch(name: string, option: string): boolean {
  const flag = option.toLowerCase();
  if (POWERSHELL.has(name)) return flag === "-command" || flag === "-c";
  if (POSIX_SHELLS.has(name)) return /^-[a-z]*c$/.test(flag);
  return name === "cmd" && (flag === "/c" || flag === "/k");
}

function innerCommand(parsed: Parsed): string | null {
  const name = executableName(parsed.tokens[0] ?? "").toLowerCase();
  if (!POWERSHELL.has(name) && !POSIX_SHELLS.has(name) && name !== "cmd") return null;
  for (let i = 1; i < parsed.tokens.length; i++) {
    if (!isCommandSwitch(name, parsed.tokens[i])) continue;
    const inner = POSIX_SHELLS.has(name) ? parsed.tokens[i + 1] ?? "" : parsed.rest(i + 1);
    return inner.trim() ? unquoteWhole(inner) : null;
  }
  return null;
}

function compact(value: string): string {
  return value.split(/\r?\n/, 1)[0].replace(/\s+/g, " ").trim();
}

function clip(value: string): string {
  const chars = Array.from(value);
  if (chars.length <= MAX_LABEL_CHARS) return value;
  const head = chars.slice(0, MAX_LABEL_CHARS - 1).join("");
  const boundary = head.lastIndexOf(" ");
  return (boundary > 30 ? head.slice(0, boundary) : head).trimEnd() + "…";
}

function unwrapCommand(command: unknown, depth = 0): string {
  let parsed: Parsed;
  if (Array.isArray(command) && command.length > 0 && command.every((part) => typeof part === "string")) {
    parsed = argvParsed(command as string[]);
  } else if (typeof command === "string") {
    parsed = tokenize(unquoteWhole(command).replace(/^&\s+/, ""));
  } else {
    return "";
  }
  if (parsed.tokens.length === 0) return "";
  const inner = innerCommand(parsed);
  if (inner !== null && depth < 8) return unwrapCommand(inner, depth + 1);
  const source = parsed.rest(0);
  const stripped = source.replace(/^(?:Set-Location|cd)\s+(?:"[^"]*"|'[^']*'|[^;&\r\n]+)\s*(?:&&|;)\s*/i, "");
  if (stripped !== source && depth < 8) return unwrapCommand(stripped, depth + 1);
  const rest = parsed.rest(1);
  return compact(`${executableName(parsed.tokens[0])}${rest ? " " + rest : ""}`);
}

export interface ActivityTool {
  toolName?: string | null;
  toolKind?: string | null;
  toolTitle?: string | null;
  toolSummary?: string | null;
  toolDetail?: string | null;
  toolInput?: unknown;
  fileDiffs?: unknown;
}

function record(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

export function activityCommand(tool: ActivityTool): string {
  const input = record(tool.toolInput);
  const command = input.cmd ?? input.command ?? tool.toolDetail ?? (tool.toolKind === "command" ? tool.toolSummary : undefined);
  return Array.isArray(command) ? argvParsed(command.map(String)).rest(0) : typeof command === "string" ? command : "";
}

function basename(value: string): string {
  return value.split(/[\\/]/).pop() || "file";
}

export function activityLabel(tool: ActivityTool): string {
  const input = record(tool.toolInput);
  const name = tool.toolName ?? "";
  const file = (Array.isArray(tool.fileDiffs) ? record(tool.fileDiffs[0]).path : undefined) ?? input.file_path ?? input.path;
  if (tool.toolKind === "edit" || /apply_patch|^(?:Edit|Write|MultiEdit)$/i.test(name)) {
    const patch = typeof tool.toolInput === "string" ? tool.toolInput : String(input.patch ?? input.input ?? "");
    const path = typeof file === "string" ? file : patch.match(/\*\*\* (?:Update|Add|Delete) File: (.+)/)?.[1] ?? tool.toolSummary;
    return path ? clip(`Editing ${basename(path.split(/\r?\n/)[0])}`) : "Editing files";
  }
  if (tool.toolKind === "read" || /^(?:Read|read_file)$/i.test(name)) {
    return typeof file === "string" ? clip(`Reading ${basename(file)}`) : "Reading files";
  }
  if (tool.toolKind !== "command" && input.cmd === undefined && input.command === undefined) {
    return clip(compact(tool.toolTitle || name || "Working"));
  }
  const command = activityCommand(tool);
  if (!command) return clip(compact(tool.toolTitle || name || "Working"));
  const unwrapped = unwrapCommand(command);
  const parsed = tokenize(unwrapped);
  let tokens = parsed.tokens;
  if (POSIX_SHELLS.has(executableName(tokens[0] ?? "")) && /build\.(?:sh|ps1)$/i.test(tokens[1] ?? "")) tokens = tokens.slice(1);
  if (["npx", "pnpm", "npm", "yarn", "bun"].includes(executableName(tokens[0] ?? "").toLowerCase())) {
    tokens = tokens.slice(1);
    if (["exec", "run", "x"].includes(tokens[0])) tokens = tokens.slice(1);
  }
  const exe = executableName(tokens[0] ?? "").toLowerCase();
  if (exe === "tsc") return "Checking types";
  if (["vitest", "jest", "test"].includes(exe) || (exe === "node" && tokens.includes("--test"))) return "Running tests";
  if (exe === "build.sh" || exe === "build.ps1") return clip(`Build: ${tokens[1] || "build"}`);
  if (exe === "git") {
    let index = 1;
    while (tokens[index]?.startsWith("-")) index += ["-C", "-c", "--git-dir", "--work-tree"].includes(tokens[index]) ? 2 : 1;
    return clip(`git ${tokens[index] || ""}`.trim());
  }
  if (exe === "apply_patch") {
    const path = command.match(/\*\*\* (?:Update|Add|Delete) File: ([^\r\n]+)/)?.[1];
    return path ? clip(`Editing ${basename(path)}`) : "Editing files";
  }
  if (exe === "rg" || exe === "grep") return "Searching";
  if (["cat", "get-content"].includes(exe) || (exe === "sed" && tokens[1] === "-n")) {
    const args = tokens.slice(exe === "sed" ? 3 : 1);
    const path = args.find(token => !token.startsWith("-"));
    return path ? clip(`Reading ${basename(path)}`) : "Reading files";
  }
  return clip(unwrapped) || "Working";
}
