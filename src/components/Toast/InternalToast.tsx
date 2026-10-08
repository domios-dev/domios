import { useEffect, useRef } from "react";
import { useMultiplexer } from "../../store/multiplexer";

interface InternalToastProps {
  pendingPrompt: { question: string } | null;
  tabId: string;
  agentType: string | null;
}

export function InternalToast({ pendingPrompt, tabId, agentType }: InternalToastProps) {
  const { upsertToast } = useMultiplexer();
  const presented = useRef<string | null>(null);
  useEffect(() => {
    if (!pendingPrompt || !agentType) { presented.current = null; return; }
    const key = JSON.stringify([tabId, pendingPrompt.question]);
    if (presented.current === key) return;
    presented.current = key;
    upsertToast({ id: `prompt-${tabId}`, key, tab_id: tabId, workspace_id: null, title: pendingPrompt.question, body: null, severity: "Info", level: "action_needed", lifetime_ms: 8000 });
  }, [pendingPrompt, tabId, agentType, upsertToast]);
  return null;
}
