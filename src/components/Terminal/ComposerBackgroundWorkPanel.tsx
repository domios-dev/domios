import { useId, useRef, useState } from "react";
import { Bot, ChevronDown, Square, SquareTerminal } from "lucide-react";
import { useAgentInspection } from "@codeterm/chat-ui";
import type { ComposerBackgroundWorkItem } from "./composerBackgroundWorkModel";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { subagentTone } from "./childTranscriptViewModel";

function SubagentDetails({ items, internalAgentLabel }: { items: readonly ComposerBackgroundWorkItem[]; internalAgentLabel: string }) {
  const inspect = useAgentInspection();
  if (!items.length) return null;
  return <ul aria-label="Subagents" className="ct-background-commands">
    {items.map(item => <li key={item.id} className="ct-background-command">
      <span className="ct-ctx-bubble" data-tone={subagentTone(item.status)}>
      <button type="button" className="ct-ctx-bubble__open" aria-label={`Open transcript for ${item.name}, ${internalAgentLabel}`} disabled={!inspect || !item.transcriptThreadId} title={[internalAgentLabel, item.prompt, item.model, item.effort, item.status === "unknown" ? "State not reported" : item.status, item.elapsed && `${item.elapsed} elapsed`].filter(Boolean).join(" · ")} onClick={() => { if (item.transcriptThreadId) inspect?.(item.transcriptThreadId); }}>
        <Bot size={13} aria-hidden="true" />
        <span className="ct-ctx-bubble__title">{item.name}</span>
      </button>
      </span>
      <div className="ct-background-command__details">
        {item.prompt && <div>{item.prompt}</div>}
        <div>{[item.model, item.effort, !item.status || item.status === "unknown" ? "State not reported" : item.status, item.elapsed && `${item.elapsed} elapsed`].filter(Boolean).join(" · ")}</div>
      </div>
    </li>)}
  </ul>;
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

export function ComposerBackgroundWorkPanel({ items, onStop, internalAgentLabel }: {
  items: readonly ComposerBackgroundWorkItem[];
  internalAgentLabel: string;
  onStop: (item: ComposerBackgroundWorkItem) => Promise<void>;
}) {
  return <div>
    <SubagentDetails items={items.filter(item => item.kind === "agent")} internalAgentLabel={internalAgentLabel} />
    <ul className="ct-background-commands">
      {items.filter(item => item.kind === "terminal").map(item => <WorkDetails key={item.id} item={item} onStop={onStop} />)}
    </ul>
  </div>;
}
