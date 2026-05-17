/**
 * Codex CLI provider plugin — entry point.
 *
 * Codex doesn't have a JSONL session format we currently parse on the
 * frontend (the Rust side reads its `rollout-*.jsonl` files for
 * session search but the live chat just uses xterm). So this plugin
 * contributes:
 *   - title detection
 *   - PTY fingerprint detection
 *   - TUI noise patterns
 *   - no-op parseLine (returns no structured events)
 *   - statusOf that ignores all events (status comes from PTY parser
 *     on the Rust side)
 *   - Renderer that falls back to raw xterm
 *
 * When we land a fancy Codex chat renderer we extend parseLine /
 * statusOf without touching the host.
 */

import type {
  ProviderPlugin,
  ProviderEvent,
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
    // Codex prints `codex/` in its prompt frame and has a distinct
    // "▶ Provide your answer" affordance.
    if (sample.bytes.includes("codex/")) return true;
    return false;
  },
  parseLine: (_line: string): ProviderEvent[] => [],
  statusOf: () => null,
  Renderer,
  actions: [],
  noisePatterns,
  titlePattern,
};

export default plugin;
