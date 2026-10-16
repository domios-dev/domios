import { Activity, Bot, Terminal } from "lucide-react";
import { useRef, useState } from "react";
import { useAgentInspection } from "@codeterm/chat-ui";
import { ComposerContextChip } from "./ComposerContextChip";
import { useRuntimeStatusPopover } from "./useRuntimeStatusPopover";
import { composerWorkBadgeLayout, type ComposerBackgroundWorkItem } from "./composerBackgroundWorkModel";
import { InlineErrorNotice } from "../shared/ErrorNotice";

function WorkDetails({ item, onStop, onInspect }: { item: ComposerBackgroundWorkItem; onStop: (item: ComposerBackgroundWorkItem) => Promise<void>; onInspect?: (threadId: string) => void }) {
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
    {item.kind === "agent" ? <>
      <p className="m-0 whitespace-pre-wrap break-words text-xs text-[var(--ct-fg)]">{item.prompt}</p>
      <p className="m-0 text-[11px] text-[var(--ct-muted-fg)]">{[item.model, item.effort, item.status].filter(Boolean).join(" · ")}</p>
      {onInspect && <button type="button" className="mt-1 underline" onClick={() => onInspect(item.threadId)}>Open transcript</button>}
    </> : <>
      <code className="block whitespace-pre-wrap break-all text-xs text-[var(--ct-fg)]">{item.command}</code>
      {item.outputTail && <pre className="m-0 mt-1 max-h-28 overflow-auto whitespace-pre-wrap break-words text-[11px] text-[var(--ct-muted-fg)]">{item.outputTail}</pre>}
      <button type="button" className="mt-1 underline" aria-label={`Stop ${item.name}`} disabled={busy} onClick={() => void stop()}>{busy ? "Stopping" : "Stop"}</button>
      {error && <div role="alert" className="mt-1"><InlineErrorNotice error={error} operation="stopping the background terminal" /></div>}
    </>}
  </li>;
}

function WorkBadge({ items, onStop, onInspect }: { items: readonly ComposerBackgroundWorkItem[]; onStop: (item: ComposerBackgroundWorkItem) => Promise<void>; onInspect?: (threadId: string) => void }) {
  const { open, triggerRef, popoverRef, toggle } = useRuntimeStatusPopover();
  const grouped = items.length > 1;
  const item = items[0];
  const label = grouped ? `${items.length} running items` : `${item?.kind === "agent" ? "Subagent" : "Terminal"} ${item?.name ?? ""}`;
  const Icon = grouped ? Activity : item?.kind === "agent" ? Bot : Terminal;
  return <span className="relative inline-flex">
    <ComposerContextChip
      ref={triggerRef}
      kind="active"
      open={open}
      aria-label={label}
      aria-expanded={open}
      aria-haspopup="dialog"
      onMouseDown={(event) => event.preventDefault()}
      onFocus={() => { if (!open) toggle(); }}
      onClick={toggle}
    >
      <Icon size={12} aria-hidden="true" />
      {grouped ? <span>{items.length}</span> : <span className="max-w-24 truncate">{item?.name}</span>}
    </ComposerContextChip>
    {open && <div ref={popoverRef} role="dialog" aria-label={label} tabIndex={-1}
      className="ct-menu absolute bottom-full left-0 z-40 mb-2 w-72 max-w-[calc(100vw-2rem)] whitespace-pre-wrap break-words p-3 text-xs text-[var(--ct-fg)]">
      <ul className="m-0 list-none divide-y divide-[var(--ct-border)] p-0">
        {items.map((entry) => <WorkDetails key={entry.id} item={entry} onStop={onStop} onInspect={onInspect} />)}
      </ul>
    </div>}
  </span>;
}

export function ComposerBackgroundWorkBadges({ items, error, onStop }: {
  items: readonly ComposerBackgroundWorkItem[];
  error: string | null;
  onStop: (item: ComposerBackgroundWorkItem) => Promise<void>;
}) {
  const inspect = useAgentInspection();
  const inspectableItems = items.filter((item) => item.kind !== "agent" || inspect !== undefined);
  const layout = composerWorkBadgeLayout(inspectableItems);
  if (layout.kind === "empty" && !error) return null;
  const shown = layout.kind === "count" ? [layout.items] : layout.kind === "individual" ? layout.items.map((item) => [item]) : [];
  return <div className="ct-ctx__kinds" role="group" aria-label="Background work">
    {shown.map((entries) => <WorkBadge key={entries.map((entry) => entry.id).join(":")} items={entries} onStop={onStop} onInspect={inspect} />)}
    {error && <div role="alert"><InlineErrorNotice error={error} operation="loading background work" /></div>}
  </div>;
}
