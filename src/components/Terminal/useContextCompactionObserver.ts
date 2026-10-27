import { useEffect, useRef } from "react";
import { transcriptRows } from "@codeterm/chat-core";
import type { Tab, TabStatus } from "../../lib/types";
import { compactionPhase } from "./contextCompactionModel";
import { compactionHistoryKey, contextCompactionStore } from "./contextCompactionStore";
import { readTranscriptWindow, transcriptWindowKey } from "./transcriptStore";

export function useContextCompactionObserver(tabs: Record<string, Tab>, statuses: Record<string, TabStatus>, authoritative: boolean) {
  const observed = useRef(new Map<string, string>());
  useEffect(() => {
    if (!authoritative) return;
    for (const tab of Object.values(tabs)) {
      const session = tab.agent_session_id ?? (tab.backing?.kind === "chat_backend" ? tab.backing.session_id : null);
      if (!session) continue;
      const runtime = tab.agent_runtime_state?.state;
      const phase = compactionPhase(tab.agent_state, statuses[tab.id] === "Error" || statuses[tab.id] === "Dead"
        || runtime === "degraded" || runtime === "unloaded");
      const key = compactionHistoryKey(tab.id, session);
      if (observed.current.get(key) === phase) continue;
      observed.current.set(key, phase);
      const rows = transcriptRows(readTranscriptWindow(transcriptWindowKey(tab.id, session)).transcript);
      const tail = [...rows].reverse().find((row) => row.timelineMarker?.kind !== "context_compaction");
      void contextCompactionStore.observe(key, phase, tail?.id ?? null);
    }
  }, [tabs, statuses, authoritative]);
}
