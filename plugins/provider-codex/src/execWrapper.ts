export interface ShellCommand {
  command: string;
  workdir?: string;
  timeout_ms?: number;
}

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

function callObject(source: string): string | null {
  const start = source.indexOf("tools.shell_command");
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
    const body = callObject(raw);
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
