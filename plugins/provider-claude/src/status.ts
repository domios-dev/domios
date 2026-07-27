/**
 * Map a single Claude ProviderEvent to a coarse TabStatus the host
 * needs for badges and notifications.
 *
 * Returning `null` means "no status change — keep whatever the host
 * had before". Most events fall through to null (e.g. a tool_call
 * mid-conversation doesn't flip status).
 */

import type { TabStatus, ProviderEvent } from "../../../src/lib/plugins/types";

export function statusOf(event: ProviderEvent): TabStatus | null {
  switch (event.kind) {
    case "user_message":
      // User just sent input — agent is about to work.
      return { kind: "agent_working" };
    case "assistant_message":
    case "assistant_text":
      // Final assistant message means turn is done.
      return { kind: "agent_done" };
    case "permission_prompt":
      return {
        kind: "agent_waiting_input",
        question: (event.payload as { question?: string } | undefined)?.question,
      };
    case "tool_call":
    case "tool_result":
    case "thinking":
      // Mid-turn activity — no status change.
      return null;
    case "error":
      return {
        kind: "agent_error",
        message: (event.payload as { message?: string } | undefined)?.message,
      };
    default:
      return null;
  }
}
