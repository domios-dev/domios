/**
 * Claude renderer — consumes the parsed event stream and mounts a
 * `MessageBubble` per event. Each event's `payload` is the original
 * `ChatMessage` produced by the JSONL parser, so the bubble just
 * receives it directly.
 *
 * When `events` is empty (fresh pane, before any session content
 * loads), we fall back to `raw()` so the user still sees terminal
 * output instead of an empty box.
 */

import { MessageBubble } from "./MessageBubble";
import type { ChatMessage } from "./parser";
import type { ProviderEvent, RendererProps } from "../../../src/lib/plugins/types";

export function Renderer({ events, raw }: RendererProps) {
  if (events.length === 0) return raw();
  return (
    <div className="space-y-1.5">
      {events.map((e: ProviderEvent, i: number) => {
        const msg = e.payload as ChatMessage | undefined;
        if (!msg || typeof msg !== "object" || !("type" in msg)) return null;
        return <MessageBubble key={msg.id ?? `evt-${i}`} message={msg} />;
      })}
    </div>
  );
}
