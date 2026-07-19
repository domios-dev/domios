import { COMMAND_PATHS, type CommandPathNode } from "./commandPaths";

export interface CodetermInvocation {
  binary: string;
  path: string[]; // subcommand chain, e.g. ["agent","spawn"]
  flags: Record<string, string | true>; // --tab bda -> { tab: "bda" }
  positionals: string[];
  raw: string;
}

const BINARIES = new Set(["codeterm", "codeterm-dev"]);

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

function commandStartIndex(tokens: string[]): number {
  let i = 0;
  while (i < tokens.length && /^[A-Za-z_][A-Za-z0-9_]*=/.test(tokens[i])) i++;
  if (tokens[i] !== "env") return i;

  i++;
  while (i < tokens.length) {
    const tok = tokens[i];
    if (/^[A-Za-z_][A-Za-z0-9_]*=/.test(tok)) {
      i++;
      continue;
    }
    if ((tok === "-u" || tok === "--unset") && tokens[i + 1] != null) {
      i += 2;
      continue;
    }
    if (tok.startsWith("-")) {
      i++;
      continue;
    }
    break;
  }
  return i;
}

function executableSegments(input: string): string[] {
  const segments: string[] = [];
  let current = "";
  let quote: '"' | "'" | null = null;
  let pipelineTail = false;

  const flush = () => {
    const segment = current.trim();
    if (segment) segments.push(segment);
    current = "";
    pipelineTail = false;
  };

  for (let i = 0; i < input.length; i++) {
    const ch = input[i];
    if (quote) {
      if (!pipelineTail) current += ch;
      if (ch === quote) quote = null;
      else if ((ch === "\\" || ch === "`") && input[i + 1] != null) {
        i++;
        if (!pipelineTail) current += input[i];
      }
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      if (!pipelineTail) current += ch;
      continue;
    }
    if (ch === "\r" || ch === "\n" || ch === ";" || ((ch === "&" || ch === "|") && input[i + 1] === ch)) {
      if (input[i + 1] === ch) i++;
      flush();
      continue;
    }
    if (ch === "|") {
      pipelineTail = true;
      continue;
    }
    if (!pipelineTail) current += ch;
  }
  flush();
  return segments;
}

function codetermSegment(input: string): string | null {
  const tokens = tokenize(input);
  const i = commandStartIndex(tokens);
  return i < tokens.length && BINARIES.has(basename(tokens[i])) ? input : null;
}

export function recognize(command: string): CodetermInvocation | null {
  if (!command) return null;
  const normalized = executableSegments(command).find((part) => codetermSegment(part) != null) ?? command;

  const tokens = tokenize(normalized);
  let i = commandStartIndex(tokens);
  if (i >= tokens.length) return null;

  const binary = basename(tokens[i]);
  if (!BINARIES.has(binary)) return null;
  i++;

  const path: string[] = [];
  const flags: Record<string, string | true> = {};
  const positionals: string[] = [];
  let seenFlag = false;
  let pathNode: CommandPathNode = COMMAND_PATHS;

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
    } else if (!seenFlag && pathNode[tok]) {
      path.push(tok);
      pathNode = pathNode[tok];
    } else {
      positionals.push(tok);
    }
  }

  if (path.length === 0) return null;
  return { binary, path, flags, positionals, raw: normalized };
}
