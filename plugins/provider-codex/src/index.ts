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
    // Codex prints `codex/` in its prompt frame and has a distinct "▶ Provide your answer" affordance.
    if (sample.bytes.includes("codex/")) return true;
    return false;
  },
  statusOf: () => null,
  Renderer,
  actions: [],
  noisePatterns,
  titlePattern,
};

export default plugin;
