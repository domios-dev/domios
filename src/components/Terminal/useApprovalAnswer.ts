import { useEffect, useRef, useState } from "react";
import type { PendingPrompt } from "../../lib/types";
import { approvalHotkey, createApprovalLatch } from "./composerModels";

export function useApprovalAnswer(prompt: PendingPrompt, tabId: string, active: boolean, onRespond: (value: string) => Promise<unknown>) {
  const pending = useRef(createApprovalLatch());
  const [busy, setBusy] = useState(false);
  const answer = async (value: string) => {
    if (!pending.current.claim()) return;
    setBusy(true);
    try { await onRespond(value); } catch { /* The shared route owns resolution after an ambiguous response. */ }
  };
  useEffect(() => {
    if (!active) return;
    const handle = (event: KeyboardEvent) => {
      if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey || event.isComposing) return;
      const target = event.target;
      if (!(target instanceof HTMLElement)) return;
      const inDock = target.closest(`[data-approval-tab="${tabId}"]`);
      const inEmptyComposer = target instanceof HTMLTextAreaElement && target.dataset.tabId === tabId && !target.value;
      if (!inDock && !inEmptyComposer) return;
      const value = approvalHotkey(prompt, event.code);
      if (value === undefined) return;
      event.preventDefault();
      event.stopPropagation();
      if (!pending.current.claim()) return;
      setBusy(true);
      void onRespond(value).catch(() => {});
    };
    document.addEventListener("keydown", handle, true);
    return () => document.removeEventListener("keydown", handle, true);
  }, [active, onRespond, prompt, tabId]);
  return { answer, busy };
}
