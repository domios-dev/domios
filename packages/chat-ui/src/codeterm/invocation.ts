import { commandArgvStart, commandBasename } from "@codeterm/chat-core";
import { COMMAND_PATHS, type CommandPathNode } from "./commandPaths";

export interface CodetermInvocation {
  binary: string;
  path: string[]; // subcommand chain, e.g. ["agent","spawn"]
  flags: Record<string, string | true>; // --tab bda -> { tab: "bda" }
  positionals: string[];
  raw: string;
}

const BINARIES = new Set(["codeterm", "codeterm-dev"]);

/** A known CodeTerm CLI invocation from one command's argv, or null. */
export function invocationFromArgv(argv: readonly string[], raw: string): CodetermInvocation | null {
  let i = commandArgvStart(argv);
  if (i >= argv.length) return null;
  const binary = commandBasename(argv[i]);
  if (!BINARIES.has(binary)) return null;
  i++;

  const path: string[] = [];
  const flags: Record<string, string | true> = {};
  const positionals: string[] = [];
  let seenFlag = false;
  let pathNode: CommandPathNode = COMMAND_PATHS;

  for (; i < argv.length; i++) {
    const tok = argv[i];
    if (tok.startsWith("--")) {
      seenFlag = true;
      const eq = tok.indexOf("=");
      if (eq > 0) {
        flags[tok.slice(2, eq)] = tok.slice(eq + 1);
        continue;
      }
      const key = tok.slice(2);
      const next = argv[i + 1];
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
  return { binary, path, flags, positionals, raw };
}
