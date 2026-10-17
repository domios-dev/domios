import { useRef, useState } from "react";
import { useAgentInspection } from "@codeterm/chat-ui";
import type { ComposerBackgroundWorkItem } from "./composerBackgroundWorkModel";
import { InlineErrorNotice } from "../shared/ErrorNotice";

function WorkDetails({ item, onStop, onInspect }: {
  item: ComposerBackgroundWorkItem;
  onStop: (item: ComposerBackgroundWorkItem) => Promise<void>;
  onInspect?: (threadId: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(false);
  const transcriptThreadId = item.kind === "agent" ? item.transcriptThreadId : undefined;
  const stop = async () => {
    if (item.kind !== "terminal" || inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      await onStop(item);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  };

  return <li className="min-w-0 py-1.5">
    {item.kind === "agent" ? <>
      <p className="m-0 text-xs text-[var(--ct-fg)]">{item.name}</p>
      {item.prompt && item.prompt !== item.name && <p className="m-0 whitespace-pre-wrap break-words text-xs text-[var(--ct-fg)]">{item.prompt}</p>}
      <p className="m-0 text-[11px] text-[var(--ct-muted-fg)]">{[item.model, item.effort, item.status, item.elapsed && `${item.elapsed} elapsed`].filter(Boolean).join(" · ")}</p>
      {onInspect && transcriptThreadId && <button type="button" className="mt-1 underline" onClick={() => onInspect(transcriptThreadId)}>Open transcript</button>}
    </> : <>
      <code className="block whitespace-pre-wrap break-all text-xs text-[var(--ct-fg)]">{item.command}</code>
      <p className="m-0 text-[11px] text-[var(--ct-muted-fg)]">{item.elapsed} elapsed</p>
      {item.outputTail && <pre className="m-0 mt-1 max-h-28 overflow-auto whitespace-pre-wrap break-words text-[11px] text-[var(--ct-muted-fg)]">{item.outputTail}</pre>}
      <button type="button" className="mt-1 underline" aria-label={`Stop ${item.name}`} disabled={busy} onClick={() => void stop()}>{busy ? "Stopping" : "Stop"}</button>
      {error && <div role="alert" className="mt-1"><InlineErrorNotice error={error} operation="stopping the background terminal" /></div>}
    </>}
  </li>;
}

export function ComposerBackgroundWorkPanel({ items, onStop }: {
  items: readonly ComposerBackgroundWorkItem[];
  onStop: (item: ComposerBackgroundWorkItem) => Promise<void>;
}) {
  const inspect = useAgentInspection();
  return <ul className="m-0 list-none divide-y divide-[var(--ct-border)] p-0">
    {items.map((item) => <WorkDetails key={item.id} item={item} onStop={onStop} onInspect={inspect} />)}
  </ul>;
}
