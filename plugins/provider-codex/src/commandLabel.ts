const MAX_LABEL_CHARS = 160;
const POWERSHELL = new Set(["pwsh", "powershell"]);
const POSIX_SHELLS = new Set(["bash", "sh", "zsh"]);

interface Parsed { tokens: string[]; rest: (index: number) => string }

function executableName(token: string): string {
  const leaf = token.split(/[\\/]+/).filter(Boolean).pop() ?? token;
  return leaf.replace(/\.(exe|cmd|bat|com)$/i, "");
}

function unquoteWhole(value: string): string {
  const trimmed = value.trim();
  if (trimmed.length < 2 || !trimmed.startsWith('"') || !trimmed.endsWith('"')) return trimmed;
  try {
    const parsed: unknown = JSON.parse(trimmed);
    return typeof parsed === "string" ? parsed : trimmed;
  } catch {
    return trimmed;
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
  const display = argv.map((argument) => (argument === "" || /\s/.test(argument) ? `"${argument}"` : argument));
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
  return value.replace(/\s+/g, " ").trim();
}

function clip(value: string): string {
  const chars = Array.from(value);
  return chars.length <= MAX_LABEL_CHARS ? value : chars.slice(0, MAX_LABEL_CHARS - 3).join("").trimEnd() + "...";
}

export function commandLabel(command: unknown): string {
  let parsed: Parsed;
  if (Array.isArray(command) && command.length > 0 && command.every((part) => typeof part === "string")) {
    parsed = argvParsed(command as string[]);
  } else if (typeof command === "string") {
    parsed = tokenize(unquoteWhole(command));
  } else {
    return "";
  }
  if (parsed.tokens.length === 0) return "";
  const inner = innerCommand(parsed);
  if (inner !== null) return clip(compact(inner));
  const rest = parsed.rest(1);
  return clip(compact(`${executableName(parsed.tokens[0])}${rest ? " " + rest : ""}`));
}
