export interface ShellCommand {
  command: string;
  workdir?: string;
  timeout_ms?: number;
}

export interface ExecScriptSummary {
  kind: "command" | "read" | "generic";
  summary: string;
  detail: string;
}

const MAX_SUMMARY_CHARS = 160;

function quotedValue(source: string, key: string): string | null {
  const keyMatch = new RegExp("\\b" + key + "\\s*:\\s*").exec(source);
  if (!keyMatch) return null;
  const quoteIndex = keyMatch.index + keyMatch[0].length;
  const quote = source[quoteIndex];
  if (quote !== "\"" && quote !== "'") return null;
  let value = "";
  for (let i = quoteIndex + 1; i < source.length; i++) {
    const char = source[i];
    if (char === quote) return decodeString(value, quote);
    if (char === "\\" && i + 1 < source.length) {
      value += char + source[++i];
    } else {
      value += char;
    }
  }
  return null;
}

function decodeString(value: string, quote: string): string {
  return value.replace(/\\(.)/g, (_match, escaped: string) => {
    if (escaped === "n") return "\n";
    if (escaped === "r") return "\r";
    if (escaped === "t") return "\t";
    if (escaped === "\\" || escaped === "\"" || (quote === "'" && escaped === "'")) return escaped;
    return "\\" + escaped;
  });
}

function callObjectAt(source: string, start: number): string | null {
  const open = start < 0 ? -1 : source.indexOf("{", start);
  if (open < 0) return null;
  let depth = 0;
  let quote = "";
  for (let i = open; i < source.length; i++) {
    const char = source[i];
    if (quote) {
      if (char === "\\") i++;
      else if (char === quote) quote = "";
    } else if (char === "\"" || char === "'") {
      quote = char;
    } else if (char === "{") {
      depth++;
    } else if (char === "}" && --depth === 0) {
      return source.substring(open + 1, i);
    }
  }
  return null;
}

function callObjects(source: string, toolName: string): string[] {
  const objects: string[] = [];
  const needle = `tools.${toolName}`;
  let offset = 0;
  while (offset < source.length) {
    const start = source.indexOf(needle, offset);
    if (start < 0) break;
    const body = callObjectAt(source, start);
    if (body) objects.push(body);
    offset = start + needle.length;
  }
  return objects;
}

function scriptCommands(raw: string): string[] {
  const commandBodies = [
    ...callObjects(raw, "exec_command"),
    ...callObjects(raw, "shell_command"),
  ];
  return commandBodies
    .map((body) => quotedValue(body, "cmd") || quotedValue(body, "command") || "")
    .filter(Boolean);
}

export function commandText(value: unknown): string {
  if (typeof value === "string") return value;
  if (!Array.isArray(value) || value.length === 0 || !value.every((argument) => typeof argument === "string")) return "";
  const argv = value as string[];
  if (!argv[0].trim()) return "";
  return argv.map((argument) => argument === "" || /[\s\"]/.test(argument) ? JSON.stringify(argument) : argument).join(" ");
}

export interface CommandExecutionWrapper {
  command: string;
  detail: string;
}

export function unwrapCommandExecution(value: unknown): CommandExecutionWrapper | null {
  if (typeof value === "string") return value.trim() ? { command: value, detail: value } : null;
  if (!Array.isArray(value) || value.length < 3 || !value.every((argument) => typeof argument === "string")) return null;
  const argv = value as string[];
  for (let i = 1; i < argv.length - 1; i++) {
    const option = argv[i].toLowerCase();
    if (option !== "-command" && option !== "-c" && !/^-[a-z]*c$/.test(option)) continue;
    if (!argv[i + 1].trim()) return null;
    return { command: argv[i + 1], detail: commandText(argv) };
  }
  return null;
}

export function execScriptCommands(raw: string): string[] {
  return typeof raw === "string" ? scriptCommands(raw) : [];
}

function jsonCallObject(source: string): Record<string, unknown> | null {
  const match = /\btools\.shell_command\(\s*(\{[\s\S]*\})\s*\)\s*;/.exec(source);
  if (!match) return null;
  try {
    const value = JSON.parse(match[1]);
    return value && typeof value === "object" && !Array.isArray(value)
      ? value as Record<string, unknown>
      : null;
  } catch (_) {
    return null;
  }
}

export function unwrapShellCommand(raw: string): ShellCommand | null {
  if (typeof raw !== "string") return null;
  try {
    const value = jsonCallObject(raw);
    if (value) {
      const command = typeof value.command === "string"
        ? value.command
        : typeof value.cmd === "string" ? value.cmd : "";
      if (!command.trim()) return null;
      const workdir = typeof value.workdir === "string" && value.workdir ? value.workdir : undefined;
      const timeout_ms = typeof value.timeout_ms === "number" ? value.timeout_ms : undefined;
      return { command, ...(workdir ? { workdir } : {}), ...(timeout_ms !== undefined ? { timeout_ms } : {}) };
    }
    const body = callObjects(raw, "shell_command")[0];
    if (!body) return null;
    const command = quotedValue(body, "command") || quotedValue(body, "cmd");
    if (!command || !command.trim()) return null;
    const workdir = quotedValue(body, "workdir") || undefined;
    const timeoutText = /\btimeout_ms\s*:\s*(\d+)/.exec(body);
    const timeout_ms = timeoutText ? Number(timeoutText[1]) : undefined;
    return { command, ...(workdir ? { workdir } : {}), ...(timeout_ms !== undefined ? { timeout_ms } : {}) };
  } catch (_) {
    return null;
  }
}

function compactCommand(command: string): string {
  return command.replace(/\s+/g, " ").trim();
}

function clipSummary(value: string, limit = MAX_SUMMARY_CHARS): string {
  if (value.length <= limit) return value;
  return value.substring(0, Math.max(1, limit - 3)).trimEnd() + "...";
}

function commandSummary(commands: string[]): ExecScriptSummary {
  const compact = commands.map(compactCommand).filter(Boolean);
  const first = compact[0];
  const suffix = compact.length > 1 ? ` (+${compact.length - 1} more)` : "";
  const summary = clipSummary(first, MAX_SUMMARY_CHARS - suffix.length) + suffix;
  return { kind: "command", summary, detail: commands.map((value) => value.trim()).join("\n\n") };
}

function readableToolName(name: string): string {
  const leaf = name.split("__").pop() || name;
  return leaf
    .replace(/_/g, " ")
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

function fallbackSummary(raw: string): ExecScriptSummary {
  const nested = /\btools\.([A-Za-z_$][\w$]*)/.exec(raw);
  const firstLine = raw.split(/\r\n?|\n/).map((line) => line.trim()).find(Boolean) || "Tool activity";
  const summary = nested ? readableToolName(nested[1]) : firstLine;
  return { kind: "generic", summary: clipSummary(summary), detail: raw.trim() };
}

export function summarizeExecScript(raw: string): ExecScriptSummary | null {
  if (typeof raw !== "string" || !raw.trim()) return null;
  const commands = scriptCommands(raw);
  if (commands.length > 0) return commandSummary(commands);

  const image = callObjects(raw, "view_image")[0];
  if (image) {
    const path = quotedValue(image, "path");
    if (path) return { kind: "read", summary: clipSummary(path), detail: path };
  }

  const stdin = callObjects(raw, "write_stdin")[0];
  if (stdin) {
    const chars = quotedValue(stdin, "chars");
    const summary = chars ? "Send input to running command" : "Wait for command output";
    return { kind: "generic", summary, detail: summary };
  }
  return fallbackSummary(raw);
}
