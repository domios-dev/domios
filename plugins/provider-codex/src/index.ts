/**
 * Codex CLI provider plugin — entry point.
 *
 * Codex stores session turns in `rollout-*.jsonl`. This plugin
 * contributes:
 *   - title detection
 *   - PTY fingerprint detection
 *   - TUI noise patterns
 *   - per-line JSONL parsing for the shared chat renderer
 *   - statusOf that ignores all events (status comes from PTY parser
 *     on the Rust side)
 *   - Renderer that falls back to raw xterm for non-chat hosts
 */

import type {
  ProviderPlugin,
  RendererProps,
} from "../../../src/lib/plugins/types";

const titlePattern = /codex/i;
const noisePatterns: RegExp[] = [/^[\s─━═]+$/];
const DETECTION_THRESHOLD = 2;
const LITERAL_SIGNALS = [
  "codex/",
  "OpenAI Codex",
  "permissions: YOLO mode",
  "permissions: read-only",
  "Would you like to run the following command",
  "Would you like to make the following edits",
  "approve network access",
  "Would you like to grant these permissions",
  "codex>",
];

function outputScore(text: string): number {
  let score = 0;
  if (/^\s*(?:›|>|▶)\s+\S+/m.test(text)) score += 2;
  if (/^\s*(?:\[[^\]]+\]\s*)?codex[/>]/im.test(text)) score += 1;
  if (/ctrl\+c to cancel/i.test(text)) score += 1;
  for (const literal of LITERAL_SIGNALS) {
    if (text.includes(literal)) score += 1;
  }
  return score;
}

function Renderer({ raw }: RendererProps) {
  return raw();
}

const plugin: ProviderPlugin = {
  meta: {
    id: "codex",
    displayName: "Codex",
  },
  detect: (sample) => {
    if (sample.title && titlePattern.test(sample.title)) return true;
    return outputScore(sample.bytes) >= DETECTION_THRESHOLD;
  },
  statusOf: () => null,
  Renderer,
  actions: [],
  noisePatterns,
  titlePattern,
};

export default plugin;
