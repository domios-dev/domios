/**
 * Claude Code provider plugin — entry point.
 *
 * Exports a single `ProviderPlugin` default that the host registers at
 * startup. This plugin intentionally avoids session-line parsing here; host-side
 * chat parsing remains authoritative.
 */

import type {
  ProviderPlugin,
  DetectionSample,
} from "../../../src/lib/plugins/types";

import { detect, titlePattern, noisePatterns } from "./detect";
import { Renderer } from "./renderer";
import { statusOf } from "./status";

const plugin: ProviderPlugin = {
  meta: {
    id: "claude",
    displayName: "Claude Code",
  },
  detect: (sample: DetectionSample) => detect(sample),
  statusOf,
  Renderer,
  actions: [
    // Bare-bones for now. Provider-specific slash commands the user can trigger from tab chrome go here.
    {
      id: "compact",
      label: "/compact",
      run: () => "/compact\n",
    },
    {
      id: "exit",
      label: "/exit",
      run: () => "/exit\n",
    },
  ],
  noisePatterns,
  titlePattern,
  imageSend: {
    // Use OS clipboard paste for image insertion; raw file paths are not treated as images.
    method: "clipboard-paste",
    pasteKeystroke: "\x16",
  },
};

export default plugin;
