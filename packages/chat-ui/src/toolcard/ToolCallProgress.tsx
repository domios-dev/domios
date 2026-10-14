import { useEffect, useState } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { toolCallProgress } from "./toolCallProgressModel";

export function ToolCallProgress({message}: {message: NormalizedChatMessage}) {
  const [now, setNow] = useState(Date.now);
  const running = message.toolStatus === "running";
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [running]);
  return <span role="timer" aria-label="Tool call progress">{toolCallProgress(message, now)}</span>;
}
