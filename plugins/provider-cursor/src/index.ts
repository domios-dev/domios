/**
 * Cursor (cursor-agent) provider plugin — entry point.
 *
 * cursor-agent is a PTY interactive agent CLI (like codex). Its session
 * store is local sqlite under ~/.cursor/chats; the live chat is xterm.
 * This plugin contributes:
 *   - title detection
 *   - PTY fingerprint detection (structural TUI signals)
 *   - TUI noise patterns
 *   - statusOf that ignores all events (status comes from the PTY parser
 *     on the Rust side)
 *   - Renderer that falls back to raw xterm for non-chat hosts
 */

import type {
  ProviderPlugin,
  RendererProps,
} from "../../../src/lib/plugins/types";

const titlePattern = /cursor/i;
const noisePatterns: RegExp[] = [/^[\s─━═│┃╭╮╯╰]+$/];
const DETECTION_THRESHOLD = 2;

function outputScore(text: string): number {
  let score = 0;
  if (text.includes("Cursor Agent")) score += 1;
  if (text.includes("Workspace Trust Required")) score += 2;
  if (text.includes("Cursor Agent can execute code")) score += 2;
  if (text.includes("Run Everything")) score += 1;
  if (text.includes("Type ? in the prompt bar")) score += 1;
  if (text.includes("Plan, search, build anything")) score += 1;
  return score;
}

function Renderer({ raw }: RendererProps) {
  return raw();
}

const plugin: ProviderPlugin = {
  meta: {
    id: "cursor",
    displayName: "Cursor",
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
