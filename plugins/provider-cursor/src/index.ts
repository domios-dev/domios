/**
 * Cursor (cursor-agent) provider plugin entry point. A PTY interactive agent CLI
 * (sqlite session store under ~/.cursor/chats); contributes title/fingerprint
 * detection, TUI noise patterns, an event-ignoring statusOf, and an xterm-fallback renderer.
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
