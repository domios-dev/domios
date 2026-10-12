import { useState, type ReactNode } from "react";
import { MessageTime, type MessageTimeSide } from "./MessageTime";

export function MessageTimeFrame({ timestamp, side, children }: { timestamp?: string; side: MessageTimeSide; children: ReactNode }) {
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  return (
    <div
      className="ct-chat-message-row w-full min-w-0"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onFocus={() => setFocused(true)}
      onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFocused(false); }}
    >
      <div className="relative w-full max-w-4xl min-w-0 mx-auto">
        {(hovered || focused) && <MessageTime timestamp={timestamp} side={side} />}
        {children}
      </div>
    </div>
  );
}
