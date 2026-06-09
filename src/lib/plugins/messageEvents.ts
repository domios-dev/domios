import type { ProviderEvent } from "./types";

// Canonical ChatMessage.type -> ProviderEvent.kind mapping, shared by every
// provider plugin. Lives here (not per-plugin) so the host's `kind` vocabulary
// has one source of truth and providers cannot silently diverge.
export function chatMessageTypeToEventKind(type: string): string {
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

interface ParsedChatMessage {
  type: string;
  timestamp?: string;
}

export function messagesToEvents<T extends ParsedChatMessage>(messages: T[]): ProviderEvent[] {
  return messages.map((m) => ({
    kind: chatMessageTypeToEventKind(m.type),
    ts: m.timestamp ? Date.parse(m.timestamp) || undefined : undefined,
    payload: m,
  }));
}

// Build a `parseLine` from a provider's own full-session JSONL parser: skip
// blank lines, parse, map to events. The only provider-specific part is the
// session parser itself.
export function makeLineParser<T extends ParsedChatMessage>(
  parseSession: (line: string) => T[],
): (line: string) => ProviderEvent[] {
  return (line: string) => (line.trim() ? messagesToEvents(parseSession(line)) : []);
}
