/**
 * sst/opencode provider plugin — entry point.
 *
 * Like Codex, opencode has its own session storage that the Rust side
 * indexes for search but the live chat is xterm. Plugin contributes
 * title detection, TUI noise patterns, and falls back to raw rendering.
 */

import type {
  ProviderPlugin,
  ProviderEvent,
  RendererProps,
} from "../../../src/lib/plugins/types";

const titlePattern = /opencode/i;
const noisePatterns: RegExp[] = [/^[\s─━═█▀▄▌▐]+$/];

function Renderer({ raw }: RendererProps) {
  return raw();
}

const plugin: ProviderPlugin = {
  meta: {
    id: "opencode",
    displayName: "OpenCode",
  },
  detect: (sample) => {
    if (sample.title && titlePattern.test(sample.title)) return true;
    // opencode banners contain "OPENCODE" ASCII art and the lowercased command name in its status bar.
    if (sample.bytes.includes("opencode") || sample.bytes.includes("OPENCODE")) {
      return true;
    }
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
