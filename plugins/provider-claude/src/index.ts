/**
 * Claude Code provider plugin — entry point.
 *
 * Exports a single `ProviderPlugin` default that the host registers at
 * startup. Also re-exports parser primitives so legacy consumers in
 * `src/components/ChatView/parser.ts` keep working unchanged through a
 * thin re-export shim.
 */

import type {
  ProviderPlugin,
  ProviderEvent,
  DetectionSample,
} from "../../../src/lib/plugins/types";

import { detect, titlePattern, noisePatterns } from "./detect";
import { parseJsonlSession } from "./parser";
import { Renderer } from "./renderer";
import { statusOf } from "./status";

// Per-line adapter: plugin contract parses one JSONL line at a time.
// Parse full-session ownership stays in the plugin while messages are emitted as ProviderEvents.

function parseLine(line: string): ProviderEvent[] {
  if (!line.trim()) return [];
  const messages = parseJsonlSession(line);
  return messages.map(m => ({
    kind: chatMessageTypeToEventKind(m.type),
    ts: m.timestamp ? Date.parse(m.timestamp) || undefined : undefined,
    payload: m,
  }));
}

function chatMessageTypeToEventKind(type: string): string {
  // Keep legacy ChatMessage.type → ProviderEvent.kind mapping for stable downstream pattern matching.
  switch (type) {
    case "user":
      return "user_message";
    case "assistant":
      return "assistant_text";
    case "tool_call":
      return "tool_call";
    case "tool_result":
      return "tool_result";
    case "thinking":
      return "thinking";
    case "status":
      return "status";
    case "system":
      return "system";
    case "local_command":
      return "local_command";
    case "image":
      return "assistant_image";
    default:
      return type;
  }
}

const plugin: ProviderPlugin = {
  meta: {
    id: "claude",
    displayName: "Claude Code",
  },
  detect: (sample: DetectionSample) => detect(sample),
  parseLine,
  statusOf,
  Renderer,
  actions: [
    // Bare-bones for now. Provider-specific slash commands the user can trigger from pane chrome go here.
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

// Re-exports for legacy consumers (src/components/ChatView/parser.ts shim).
export * from "./parser";
