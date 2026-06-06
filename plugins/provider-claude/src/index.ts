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

// ── Per-line adapter ─────────────────────────────────────────────
//
// The original Claude parser was designed to process a whole JSONL
// file at once (it carries per-message indexing for stable ids). For
// the plugin contract we expose a per-line `parseLine` that runs the
// same parser on one line and returns whichever ChatMessages it
// produced, wrapped as ProviderEvents. The `payload` carries the full
// ChatMessage so renderers don't lose any fidelity.

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
  // Map the legacy ChatMessage.type → plugin-API ProviderEvent.kind.
  // Keep names stable so status.ts and downstream consumers can
  // pattern-match without coupling to renderer internals.
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
    // Bare-bones for now. Provider-specific slash commands the user
    // can trigger from pane chrome go here.
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
    // Claude Code reads from the OS clipboard when it sees Ctrl+V
    // (0x16). Sending the file path as raw text doesn't trigger image
    // upload — clipboard is the only reliable channel.
    method: "clipboard-paste",
    pasteKeystroke: "\x16",
  },
};

export default plugin;

// Re-exports for legacy consumers (src/components/ChatView/parser.ts shim).
export * from "./parser";
