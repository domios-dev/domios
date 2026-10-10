import { useState } from "react";
import type { PendingPrompt } from "../../lib/types";
import { useApprovalAnswer } from "./useApprovalAnswer";
import { approvalActions } from "./composerModels";

export function ComposerApprovalDock({ prompt, tabId, active, onRespond }: { prompt: PendingPrompt; tabId: string; active: boolean; onRespond: (value: string) => Promise<unknown> }) {
  const { answer, busy } = useApprovalAnswer(prompt, tabId, active, onRespond);
  const [overflow, setOverflow] = useState(false);
  const actions = approvalActions(prompt);
  const primary = actions.filter((option) => option.rank < 2);
  const extra = actions.filter((option) => option.rank >= 2);
  return <section aria-label="Approval dock" data-approval-tab={tabId} className="mb-2 rounded-lg border p-3">
    <div className="flex items-center justify-between text-xs"><span>Permission</span><span>1/1</span></div>
    <pre className="my-2 max-h-32 overflow-auto whitespace-pre-wrap text-xs">{prompt.question}</pre>
    <div className="flex flex-wrap items-center gap-2">
      {primary.map((option, index) => <button type="button" key={option.value} disabled={busy}
        className={index === 0 ? "rounded bg-[var(--ct-accent)] px-3 py-1 text-[var(--ct-bg)]" : "rounded border px-3 py-1"}
        onClick={() => void answer(option.value)}>{option.hotkey} · {option.label}</button>)}
      {extra.length > 0 && <button type="button" aria-label="More approval options" aria-expanded={overflow} onClick={() => setOverflow(!overflow)}>…</button>}
      {overflow && extra.map((option) => <button type="button" key={option.value} disabled={busy} onClick={() => void answer(option.value)}>{option.hotkey} · {option.label}</button>)}
    </div>
  </section>;
}
