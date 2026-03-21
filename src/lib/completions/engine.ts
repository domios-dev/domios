import type { CompletionDefinition, CompletionItem, ParsedInput, SubcommandDef, FlagDef } from "./types";

export class CompletionEngine {
  private definitions = new Map<string, CompletionDefinition>();

  register(def: CompletionDefinition): void {
    this.definitions.set(def.command, def);
  }

  hasDefinition(command: string): boolean {
    return this.definitions.has(command);
  }

  getCompletions(parsed: ParsedInput): CompletionItem[] {
    const def = this.definitions.get(parsed.command);
    if (!def) return [];

    // Walk subcommand tree based on args
    let context: { subcommands?: SubcommandDef[]; flags?: FlagDef[] } = def;
    for (const arg of parsed.args) {
      if (arg.startsWith("-")) continue; // skip flags in path
      const sub = context.subcommands?.find((s) => s.name === arg);
      if (sub) {
        context = sub;
      } else {
        break;
      }
    }

    const prefix = parsed.currentArg.toLowerCase();
    const items: CompletionItem[] = [];

    // If prefix starts with "-", show flags
    if (prefix.startsWith("-") || prefix === "") {
      if (context.flags) {
        for (const flag of context.flags) {
          if (fuzzyMatch(flag.name, prefix) || (flag.short && fuzzyMatch(flag.short, prefix))) {
            items.push({
              text: flag.name,
              display: flag.short ? `${flag.name}, ${flag.short}` : flag.name,
              description: flag.description,
              kind: "flag",
            });
          }
        }
      }
    }

    // Show subcommands (if not typing a flag)
    if (!prefix.startsWith("-") && context.subcommands) {
      for (const sub of context.subcommands) {
        if (fuzzyMatch(sub.name, prefix)) {
          items.push({
            text: sub.name,
            display: sub.name,
            description: sub.description,
            kind: "subcommand",
          });
        }
      }
    }

    // Show both when empty prefix
    if (prefix === "" && context.subcommands) {
      // subcommands already added above
    }
    if (prefix === "" && context.flags) {
      // flags already added above via fuzzyMatch("--flag", "")
    }

    // Sort: exact prefix first, then fuzzy
    items.sort((a, b) => {
      const aExact = a.text.toLowerCase().startsWith(prefix) ? 0 : 1;
      const bExact = b.text.toLowerCase().startsWith(prefix) ? 0 : 1;
      if (aExact !== bExact) return aExact - bExact;
      // Subcommands before flags
      if (a.kind !== b.kind) return a.kind === "subcommand" ? -1 : 1;
      return a.text.localeCompare(b.text);
    });

    return items;
  }
}

/** Fuzzy prefix match — returns true if candidate starts with or contains prefix. */
function fuzzyMatch(candidate: string, prefix: string): boolean {
  if (!prefix) return true;
  const c = candidate.toLowerCase();
  const p = prefix.toLowerCase();

  // Exact prefix
  if (c.startsWith(p)) return true;

  // Subsequence match (for short prefixes)
  if (p.length <= 3) {
    let ci = 0;
    for (let pi = 0; pi < p.length && ci < c.length; ci++) {
      if (c[ci] === p[pi]) pi++;
      if (pi === p.length) return true;
    }
  }

  return false;
}
