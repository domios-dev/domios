// TypeScript source for the Cursor Agent provider plugin.
//
// CodeTerm's Rust-side QuickJS runtime loads `plugin.js` next to
// `plugin.json` and calls the exported functions as needed. This file is
// the editable source; run `node build.mjs` to refresh `plugin.js`
// after edits. The build is a hand-rolled string-substitution step —
// no esbuild required for plugins this small.
//
// Every exported function MUST be pure (no I/O, no fetch, no setTimeout)
// and MUST return synchronously. The host serializes args/results as
// JSON over the Rust↔QuickJS bridge.

type LaunchParams = {
  agentName: string | null;
  task: string | null;
  args: string[] | null;
  skipPermissions: boolean;
  headless: boolean;
  sessionId: string | null;
};

function shellQuote(s: string): string {
  // POSIX single-quote escape: ' -> '\''
  return "'" + s.replace(/'/g, `'\\''`) + "'";
}

const TITLE_RE = /\b(cursor[- ]?agent|cursor cli)\b/i;

// Structural fingerprints in PTY output — banner row plus the prompt
// arrow Cursor draws above the input box. We avoid version-tagged
// strings (would silently break on upgrade) and stick to glyphs that
// are unique enough vs. plain shells.
const OUTPUT_FINGERPRINTS = [
  "Cursor Agent",
  "/plan",
  "/ask",
  "Max Mode",
];

const NOISE_FRAGMENTS = [
  "ctrl+c to exit",
  "loading...",
  "thinking...",
];

export default {
  detectFromTitle(title: string): boolean {
    return TITLE_RE.test(title);
  },

  detectFromOutput(text: string): boolean {
    return OUTPUT_FINGERPRINTS.some((f) => text.includes(f));
  },

  screenHasTui(screen: string): boolean {
    // Cursor draws a bordered input box at the bottom and a status bar
    // line — pick any of the structural strings that don't appear in
    // plain shells.
    const lower = screen.toLowerCase();
    return (
      lower.includes("max mode") ||
      lower.includes("/plan") ||
      lower.includes("/ask")
    );
  },

  isOutputNoise(text: string): boolean {
    const lower = text.toLowerCase();
    return NOISE_FRAGMENTS.some((n) => lower.includes(n));
  },

  hasEventMarkers(chunk: string): boolean {
    // Approval prompts use a numbered list ("1. Yes  2. No") in Cursor;
    // generic prompt-bullet "❯" appears for selection arrows.
    return (
      chunk.includes("❯") ||
      (chunk.includes("1.") && chunk.includes("2.")) ||
      chunk.toLowerCase().includes("allow this command")
    );
  },

  // Cursor CLI: `agent` interactive; `-p "<task>"` non-interactive print;
  // `--resume <chat-id>` to resume; `--continue` for the last session;
  // `--model <id>` to pick a model; `--mode plan|ask` (--plan shorthand);
  // `--sandbox enabled|disabled`. No --dangerously-skip equivalent
  // exists — sandbox=disabled is the closest analogue, so that's what
  // `skipPermissions` maps to.
  buildLaunchCommand(params: LaunchParams): string {
    const parts: string[] = ["agent"];

    if (params.headless && params.task) {
      parts.push("-p");
      parts.push(shellQuote(params.task));
      parts.push("--output-format", "text");
    }

    if (params.skipPermissions) {
      parts.push("--sandbox", "disabled");
    }

    if (params.args && params.args.length > 0) {
      for (const a of params.args) parts.push(shellQuote(a));
    }

    return parts.join(" ");
  },

  buildResumeCommand(sessionId: string, skipPermissions: boolean): string {
    const parts = ["agent", "--resume", shellQuote(sessionId)];
    if (skipPermissions) parts.push("--sandbox", "disabled");
    return parts.join(" ");
  },

  // Cursor's interactive prompt is the standard numbered list. We don't
  // override `buildPromptResponse` — the trait default (arrow-down +
  // Enter) is correct for this TUI.

  parsePrompt(text: string): { question: string; options: Array<[string, string]> } | null {
    // Look for "Allow this command?" or generic "1. <label>  2. <label>"
    const lines = text.split("\n");
    let qLine = "";
    const opts: Array<[string, string]> = [];
    for (const raw of lines) {
      const line = raw.trim();
      const m = line.match(/^(\d+)\.\s+(.+)$/);
      if (m) {
        opts.push([m[1], m[2].trim()]);
      } else if (line.endsWith("?") && !qLine) {
        qLine = line;
      }
    }
    if (opts.length < 2 || !qLine) return null;
    return { question: qLine, options: opts };
  },
};
