import { useId, useRef, useState } from "react";
import { Bot, ChevronDown, Square, SquareTerminal } from "lucide-react";
import { useAgentInspection } from "@codeterm/chat-ui";
import type { ComposerBackgroundWorkItem } from "./composerBackgroundWorkModel";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { subagentTone } from "./childTranscriptViewModel";

const NO_LIVE_EVIDENCE = "No live evidence";

function stateLabel(item: ComposerBackgroundWorkItem): string {
  if (item.status === "unknown" || !item.status) return "unknown";
  return item.elapsed ? `running · ${item.elapsed}` : "running";
}

function displayName(item: ComposerBackgroundWorkItem): string {
  return item.ordinal ? `${item.name} ${item.ordinal}` : item.name;
}

function describe(item: ComposerBackgroundWorkItem): string {
  return [`Task ${displayName(item)}, ${item.status === "unknown" ? NO_LIVE_EVIDENCE.toLowerCase() : stateLabel(item)}`, item.model, item.effort, item.rawId && `id ${item.rawId}`].filter(Boolean).join(". ");
}

function WorkState({ item, label = stateLabel(item) }: { item: ComposerBackgroundWorkItem; label?: string }) {
  const unknown = item.status === "unknown" || !item.status;
  return <span className="ct-work-state" data-state={unknown ? "unknown" : "running"} title={unknown ? NO_LIVE_EVIDENCE : undefined}>
    <span className="ct-work-state__dot" aria-hidden="true" />{label.replace(/^running · /, "")}
  </span>;
}

function SubagentRow({ item, internalAgentLabel }: { item: ComposerBackgroundWorkItem; internalAgentLabel: string }) {
  const inspect = useAgentInspection();
  const [expanded, setExpanded] = useState(false);
  const promptId = useId();
  return <li className="ct-background-command">
    <span className="ct-ctx-bubble ct-work-bubble" data-tone={subagentTone(item.status)}>
      <button type="button" className="ct-ctx-bubble__open" aria-label={`Open transcript for ${item.name}, ${internalAgentLabel}`} aria-description={describe(item)}
        disabled={!inspect || !item.transcriptThreadId} title={`${describe(item)} · ${internalAgentLabel}`} onClick={() => { if (item.transcriptThreadId) inspect?.(item.transcriptThreadId); }}>
        <Bot size={13} aria-hidden="true" />
        <span className="ct-ctx-bubble__title">{item.name}</span>
        {item.ordinal && <span className="ct-work-bubble__ordinal" aria-hidden="true">{item.ordinal}</span>}
        <WorkState item={item} />
      </button>
      {item.prompt && <button type="button" className="ct-ctx-bubble__dismiss" aria-label={`Task details for ${item.name}`} aria-expanded={expanded} aria-controls={promptId} title="Task details" onClick={() => setExpanded(!expanded)}>
        <ChevronDown size={11} className={expanded ? "rotate-180" : undefined} aria-hidden="true" />
      </button>}
    </span>
    {expanded && item.prompt && <div id={promptId} role="region" aria-label={`Task details for ${item.name}`} className="ct-background-command__details ct-background-command__prompt">{item.prompt}</div>}
  </li>;
}

function SubagentDetails({ items, internalAgentLabel }: { items: readonly ComposerBackgroundWorkItem[]; internalAgentLabel: string }) {
  if (!items.length) return null;
  return <ul aria-label="Subagents" className="ct-background-commands">
    {items.map(item => <SubagentRow key={item.id} item={item} internalAgentLabel={internalAgentLabel} />)}
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
      <span className="ct-ctx-bubble ct-work-bubble" data-tone="active">
        <button type="button" className="ct-ctx-bubble__open" aria-label={`Details for ${item.name}`} aria-description={`Task ${item.name}, ${busy ? "stopping" : stateLabel(item)}`} aria-expanded={expanded} aria-controls={detailsId} title={item.command} onClick={() => setExpanded(!expanded)}>
          <SquareTerminal size={13} aria-hidden="true" />
          <span className="ct-ctx-bubble__title">{item.name}</span>
          <WorkState item={item} label={busy ? "stopping" : stateLabel(item)} />
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
