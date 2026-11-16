import { useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { promptResponseArgs } from "../../lib/promptAnswers";
import type { PendingPrompt } from "../../lib/types";
import { InlineErrorNotice } from "../shared/ErrorNotice";

function useProviderQuestionAnswer(tabId: string, prompt: PendingPrompt, readOnly: boolean) {
  const answering = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function answer(value: string) {
    if (answering.current || readOnly) return;
    answering.current = true;
    setBusy(true);
    setError(null);
    try {
      await invoke("mux_respond_prompt", promptResponseArgs(tabId, value, prompt, "provider"));
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
      answering.current = false;
      setBusy(false);
    }
  }
  return { answer, busy, error };
}

export function LaunchProviderQuestion({ tabId, prompt, readOnly, onOpenTerminal }: {
  tabId: string; prompt: PendingPrompt; readOnly: boolean; onOpenTerminal?: () => void;
}) {
  const { answer, busy, error } = useProviderQuestionAnswer(tabId, prompt, readOnly);
  return <section role="region" aria-label="Agent needs your input" aria-busy={busy}
    className="absolute inset-0 z-20 flex flex-col items-center justify-center gap-4 overflow-auto bg-[var(--ct-bg)] p-6">
    <h2 className="max-w-2xl text-center text-lg font-medium">{prompt.question}</h2>
    <div className="flex max-w-2xl flex-wrap justify-center gap-3">
      {prompt.options.map(option => <button type="button" key={option.value}
        disabled={busy || readOnly} onClick={() => void answer(option.value)}
        className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-surface)] px-4 py-2">{option.label}</button>)}
    </div>
    {onOpenTerminal && <button type="button" data-terminal-fallback-toggle="true" onClick={onOpenTerminal}>Open terminal</button>}
    {error && <InlineErrorNotice error={error} operation="answering the agent" />}
  </section>;
}
