import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { projectVisibleChatTools } from "./chatTranscriptActivity";

const NO_RUNNING_TOOLS: ReturnType<typeof projectVisibleChatTools>["running"] = new Map();

export function useVisibleChatTools(messages: NormalizedChatMessage[], scope: string | null) {
  const [revision, setRevision] = useState(0);
  const committed = useRef<{ scope: string; running: ReturnType<typeof projectVisibleChatTools>["running"] } | null>(null);
  const projection = useMemo(() => scope === null ? null : projectVisibleChatTools(
    messages, committed.current?.scope === scope ? committed.current.running : NO_RUNNING_TOOLS, Date.now(),
  ), [messages, revision, scope]);

  useLayoutEffect(() => {
    if (scope === null || !projection) {
      committed.current = null;
      return;
    }
    committed.current = { scope, running: projection.running };
    if (!Number.isFinite(projection.nextChangeAt)) return;
    const timer = window.setTimeout(() => setRevision(value => value + 1), Math.max(0, projection.nextChangeAt - Date.now()));
    return () => window.clearTimeout(timer);
  }, [projection, scope]);

  return projection?.rows ?? messages;
}
