/**
 * Per-provider surface for the stub replayer, kept out of replay.mjs so the
 * harness can assert it without spawning a process.
 *
 * Two invariants:
 * - Every provider the stub can wear owns a cast (replay-cast.mjs) AND a style.
 *   Wearing another provider's banner makes detection fingerprint the wrong one.
 * - An unknown provider gets a neutral style that names itself and carries no
 *   provider fingerprint at all — honest rather than silently Claude.
 */

/** stub-gate.ts probes tab output for this marker to decide the transport is live. */
export const STUB_MARKER = "stub replay";

const ESC = "";

function styles(model) {
  const suffix = `  model: ${model ?? "default"}`;
  return {
    claude: {
      banner: `${ESC}[38;2;217;119;87m✳${ESC}[0m Claude Code (${STUB_MARKER})${suffix}`,
      turn: (text) => `${ESC}[1m●${ESC}[0m ${text}`,
      tool: (marker) => `${ESC}[2m⏺ Bash(echo ${marker})${ESC}[0m\n${ESC}[2m  ⎿  ${marker}${ESC}[0m`,
      idle: `${ESC}[2m❯${ESC}[0m `,
    },
    codex: {
      banner: `${ESC}[36mOpenAI Codex (${STUB_MARKER})${ESC}[0m${suffix}`,
      turn: (text) => `${ESC}[1mcodex${ESC}[0m  ${text}`,
      tool: (marker) => `${ESC}[2mexec${ESC}[0m echo ${marker}\n${marker}`,
      idle: `${ESC}[2m▌${ESC}[0m `,
    },
    opencode: {
      banner: `${ESC}[35m█ opencode (${STUB_MARKER})${ESC}[0m${suffix}`,
      turn: (text) => `${ESC}[1m@${ESC}[0m ${text}`,
      tool: (marker) => `${ESC}[2m$ echo ${marker}${ESC}[0m\n${marker}`,
      idle: `${ESC}[2m>${ESC}[0m `,
    },
    // Shaped after testbench/replays/cursor/*: the boot block is "Cursor Agent",
    // a dim version line and the "Plan, search, build anything" prompt bar, which
    // is what plugins/provider-cursor fingerprints.
    cursor: {
      banner:
        `${ESC}[1m  Cursor Agent (${STUB_MARKER})${ESC}[0m${suffix}\n` +
        `  ${ESC}[2mUse /plan to iterate on an implementation plan before code changes.${ESC}[0m\n` +
        `  ${ESC}[2m→ Plan, search, build anything${ESC}[0m`,
      turn: (text) => `  ${ESC}[1m•${ESC}[0m ${text}`,
      tool: (marker) => `  ${ESC}[2mRan echo ${marker}${ESC}[0m\n  ${marker}`,
      idle: `${ESC}[2m→${ESC}[0m `,
    },
  };
}

export const KNOWN_STUB_PROVIDERS = Object.keys(styles(null));

/** Neutral surface for a provider with no recorded style: names itself, borrows nobody. */
function genericStyle(provider, model) {
  const id = String(provider || "unknown");
  return {
    banner: `${ESC}[1m${id} (${STUB_MARKER}, no recorded style)${ESC}[0m  model: ${model ?? "default"}`,
    turn: (text) => `${ESC}[1m${id}${ESC}[0m ${text}`,
    tool: (marker) => `${ESC}[2m$ echo ${marker}${ESC}[0m\n${marker}`,
    idle: `${ESC}[2m$${ESC}[0m `,
  };
}

export function styleFor(provider, model) {
  const table = styles(model);
  return Object.hasOwn(table, String(provider)) ? table[provider] : genericStyle(provider, model);
}
