import { useId, useRef, useState } from "react";
import { ChevronDown, Square, SquareTerminal } from "lucide-react";
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
  const [expanded, setExpanded] = useState(false);
  const detailsId = useId();
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

  return <li className="ct-background-command">
      <span className="ct-ctx-bubble" data-tone="active">
        <button type="button" className="ct-ctx-bubble__open" aria-label={`Details for ${item.name}`} aria-expanded={expanded} aria-controls={detailsId} onClick={() => setExpanded(!expanded)}>
          <SquareTerminal size={13} aria-hidden="true" />
          <span className="ct-ctx-bubble__title">{item.name}</span>
          <span className="ct-ctx-bubble__meta">{busy ? "Stopping" : item.status} · {item.elapsed}</span>
          <ChevronDown size={11} aria-hidden="true" />
        </button>
        <button type="button" className="ct-ctx-bubble__dismiss" aria-label={`Stop ${item.name}`} title={`Stop ${item.name}`} disabled={busy} aria-busy={busy || undefined} onClick={() => void stop()}><Square size={10} fill="currentColor" aria-hidden="true" /></button>
      </span>
      {expanded && <div id={detailsId} role="region" aria-label={`Command details for ${item.name}`} className="ct-background-command__details">
        <code>{item.command}</code>
        {item.outputTail && <pre>{item.outputTail}</pre>}
      </div>}
      {error && <div role="alert" className="mt-1"><InlineErrorNotice error={error} operation="stopping the background terminal" /></div>}
  </li>;
}

export function ComposerBackgroundWorkPanel({ items, onStop }: {
  items: readonly ComposerBackgroundWorkItem[];
  onStop: (item: ComposerBackgroundWorkItem) => Promise<void>;
}) {
  return <div>
    <SubagentCapsules items={items.filter(item => item.kind === "agent")} />
    <ul className="ct-background-commands">
      {items.filter(item => item.kind === "terminal").map(item => <WorkDetails key={item.id} item={item} onStop={onStop} />)}
    </ul>
  </div>;
}
