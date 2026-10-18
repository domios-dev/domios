import { useRef, useState } from "react";
import { useAgentInspection } from "@codeterm/chat-ui";
import type { ComposerBackgroundWorkItem } from "./composerBackgroundWorkModel";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { compactSubagents, subagentTone } from "./childTranscriptViewModel";

export function SubagentCapsules({ items }: { items: readonly ComposerBackgroundWorkItem[] }) {
  const inspect = useAgentInspection();
  const [expanded, setExpanded] = useState(false);
  const { visible, hidden } = compactSubagents(items, expanded);
  if (!items.length) return null;
  return <div role="group" aria-label="Subagents" className="ct-subagent-row">
    {visible.map(item => <span key={item.id} className="ct-ctx-bubble" data-tone={subagentTone(item.status)}>
      <button type="button" className="ct-ctx-bubble__open" aria-label={`Open transcript for ${item.name}`} disabled={!inspect || !item.transcriptThreadId} title={[item.prompt, item.model, item.effort, item.status, item.elapsed && `${item.elapsed} elapsed`].filter(Boolean).join(" · ")} onClick={() => { if (item.transcriptThreadId) inspect?.(item.transcriptThreadId); }}>
        <span className="ct-ctx-bubble__dot" aria-hidden="true" />
        <span className="ct-ctx-bubble__title">{item.name}</span>
        <span className="ct-ctx-bubble__meta">{item.status}</span>
        {item.model && <span className="ct-ctx-bubble__meta">{item.model}</span>}
      </button>
    </span>)}
    {hidden > 0 && <button type="button" className="ct-ctx-kind" aria-label={`Show ${hidden} more subagents`} onClick={() => setExpanded(true)}>+{hidden}</button>}
    {expanded && items.length > 3 && <button type="button" className="ct-ctx-kind" aria-label="Show fewer subagents" onClick={() => setExpanded(false)}>Show fewer</button>}
  </div>;
}

function WorkDetails({ item, onStop }: {
  item: ComposerBackgroundWorkItem;
  onStop: (item: ComposerBackgroundWorkItem) => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(false);
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
      <code className="block whitespace-pre-wrap break-all text-xs text-[var(--ct-fg)]">{item.command}</code>
      <p className="m-0 text-[11px] text-[var(--ct-muted-fg)]">{item.elapsed} elapsed</p>
      {item.outputTail && <pre className="m-0 mt-1 max-h-28 overflow-auto whitespace-pre-wrap break-words text-[11px] text-[var(--ct-muted-fg)]">{item.outputTail}</pre>}
      <button type="button" className="mt-1 underline" aria-label={`Stop ${item.name}`} disabled={busy} onClick={() => void stop()}>{busy ? "Stopping" : "Stop"}</button>
      {error && <div role="alert" className="mt-1"><InlineErrorNotice error={error} operation="stopping the background terminal" /></div>}
  </li>;
}

export function ComposerBackgroundWorkPanel({ items, onStop }: {
  items: readonly ComposerBackgroundWorkItem[];
  onStop: (item: ComposerBackgroundWorkItem) => Promise<void>;
}) {
  return <div>
    <SubagentCapsules items={items.filter(item => item.kind === "agent")} />
    <ul className="m-0 list-none divide-y divide-[var(--ct-border)] p-0">
      {items.filter(item => item.kind === "terminal").map(item => <WorkDetails key={item.id} item={item} onStop={onStop} />)}
    </ul>
  </div>;
}
