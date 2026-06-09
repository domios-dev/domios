// Recognize `codeterm …` shell invocations in a Bash tool command string.
// Paths validated against docs/cli-manifest.json by recognize.test.ts.

export interface CodetermInvocation {
  binary: string;
  path: string[]; // subcommand chain, e.g. ["agent","spawn"]
  flags: Record<string, string | true>; // --pane bda → { pane: "bda" }
  positionals: string[];
  raw: string;
}

const BINARIES = new Set(["codeterm", "codeterm-dev"]);

// Minimal hint so a depth-2 subcommand (e.g. `agent spawn`) is captured as the
// path while trailing words (e.g. the agent name) stay positional. Full
// validation of these against the real CLI is recognize.test.ts's job.
const KNOWN_SUBCOMMANDS: Record<string, Set<string>> = {
  agent: new Set([
    "spawn", "report", "list", "model", "models", "restart", "providers",
    "reports", "compact", "stop", "set-orchestrator", "link", "unlink", "allow-spawn",
  ]),
  mem: new Set(["save", "search", "get", "delete", "skill", "graph", "worker"]),
  workspace: new Set(["create", "list", "focus", "rename", "close", "current", "move-tab"]),
  pane: new Set(["list", "new", "focus", "close", "rename", "exec", "status", "search", "stop", "current"]),
  mesh: new Set(["send", "exec", "output", "panes", "peers", "status", "connect"]),
  cron: new Set(["create", "list", "edit", "delete", "trigger", "history"]),
  keepawake: new Set(["start", "stop", "status"]),
  plugin: new Set(["list", "install", "update", "update-system", "remove", "channel"]),
};

/** Split a shell-ish string into tokens, honoring single/double quotes. */
export function tokenize(input: string): string[] {
  const tokens: string[] = [];
  let cur = "";
  let quote: '"' | "'" | null = null;
  let has = false;
  for (let i = 0; i < input.length; i++) {
    const ch = input[i];
    if (quote) {
      if (ch === quote) quote = null;
      else cur += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
      has = true;
    } else if (ch === " " || ch === "\t" || ch === "\n") {
      if (has) {
        tokens.push(cur);
        cur = "";
        has = false;
      }
    } else {
      cur += ch;
      has = true;
    }
  }
  if (has) tokens.push(cur);
  return tokens;
}

function basename(tok: string): string {
  const slash = Math.max(tok.lastIndexOf("/"), tok.lastIndexOf("\\"));
  return slash >= 0 ? tok.slice(slash + 1) : tok;
}

export function recognize(command: string): CodetermInvocation | null {
  if (!command) return null;
  // Reject compound commands — we won't misrepresent a chained line.
  if (/[|;]|&&|\|\|/.test(command)) return null;

  const tokens = tokenize(command);
  let i = 0;
  // Strip leading KEY=val env assignments.
  while (i < tokens.length && /^[A-Za-z_][A-Za-z0-9_]*=/.test(tokens[i])) i++;
  if (i >= tokens.length) return null;

  const binary = basename(tokens[i]);
  if (!BINARIES.has(binary)) return null;
  i++;

  const path: string[] = [];
  const flags: Record<string, string | true> = {};
  const positionals: string[] = [];
  let seenFlag = false;

  for (; i < tokens.length; i++) {
    const tok = tokens[i];
    if (tok.startsWith("--")) {
      seenFlag = true;
      const eq = tok.indexOf("=");
      if (eq > 0) {
        flags[tok.slice(2, eq)] = tok.slice(eq + 1);
        continue;
      }
      const key = tok.slice(2);
      const next = tokens[i + 1];
      if (next != null && !next.startsWith("-")) {
        flags[key] = next;
        i++;
      } else {
        flags[key] = true;
      }
    } else if (tok.startsWith("-") && tok.length > 1) {
      seenFlag = true;
      flags[tok.slice(1)] = true;
    } else if (!seenFlag && path.length === 0) {
      path.push(tok); // first bare word = top command
    } else if (!seenFlag && path.length === 1 && KNOWN_SUBCOMMANDS[path[0]]?.has(tok)) {
      path.push(tok); // second word only if a known subcommand
    } else {
      positionals.push(tok);
    }
  }

  if (path.length === 0) return null;
  return { binary, path, flags, positionals, raw: command };
}
