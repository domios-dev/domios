import { useState } from "react";
import type { AgentRuntimeState } from "../../lib/agentRuntimeState";

export function hasRowControls(state: AgentRuntimeState | null | undefined): boolean {
  return Boolean(state?.notice);
}

export function RuntimeDeclaredControls({ state }: { tabId: string; state: AgentRuntimeState; parts?: "notice" }) {
  const [dismissed, setDismissed] = useState<string | null>(null);
  if (!state.notice || dismissed === state.notice) return null;
  return <div role="status" className="mx-auto mb-2 flex w-full max-w-4xl items-center gap-2 text-[12px] text-[var(--ct-muted-fg)]">
    <span className="min-w-0 flex-1">{state.notice}</span>
    <button type="button" className="shrink-0 rounded px-1.5 py-0.5 hover:text-[var(--ct-fg)]" onClick={() => setDismissed(state.notice!)}>Dismiss notice</button>
  </div>;
}
