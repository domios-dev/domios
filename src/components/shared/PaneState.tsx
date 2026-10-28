import { useState, type KeyboardEvent } from "react";
import { Copy } from "lucide-react";
import { Button } from "./Button";
import { paneStateModel, type PaneStateContext } from "./paneStateModel";
import type { ErrorNoticeAction } from "./ErrorNotice";
import { BrandState } from "../brand/BrandState";
import { brandStateModel } from "../brand/brandStateModel";
import { RegionLoader } from "./RegionLoader";

function stopActivation(event: KeyboardEvent<HTMLElement>) {
  if (event.code === "Enter" || event.code === "Space") event.stopPropagation();
}

export function PaneState({ error, context = {}, actions = [], secondaryActions = [], detailActions = [] }: {
  error?: unknown;
  context?: PaneStateContext;
  actions?: ErrorNoticeAction[];
  secondaryActions?: ErrorNoticeAction[];
  detailActions?: ErrorNoticeAction[];
}) {
  const [copyStatus, setCopyStatus] = useState("");
  const view = paneStateModel(error, context);
  const path = view.path;
  const copyDetails = async () => {
    try { await navigator.clipboard.writeText(view.details); setCopyStatus("Details copied"); }
    catch { setCopyStatus("Could not copy the details"); }
  };
  const copyPath = async () => {
    try { await navigator.clipboard.writeText(path ?? ""); setCopyStatus("Path copied"); }
    catch { setCopyStatus("Could not copy the path"); }
  };
  if (view.pending) return <RegionLoader label={view.title} className="my-2 min-h-48" />;
  return <BrandState model={brandStateModel({ error: { title: view.title, sentence: view.sentence } })} className="h-full">
    <div className="grid w-full min-w-0 max-w-md grid-cols-[minmax(0,1fr)_auto] items-center gap-3 text-center">
      {path && <div title={path} className="col-start-1 row-start-1 flex max-w-full min-w-0 rounded-md bg-[var(--ct-surface)] px-3 py-2 font-mono text-xs text-[var(--ct-muted-fg)]">
        <span className="min-w-0 truncate">{path.slice(0, Math.ceil(path.length / 2))}</span><span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap [direction:rtl]">{path.slice(Math.ceil(path.length / 2))}</span>
      </div>}
      <div className="col-span-2 row-start-2 flex flex-wrap items-center justify-center gap-2">
        {actions.map((action, index) => <Button key={action.ariaLabel ?? action.label} variant={index === 0 ? "primary" : "ghost"} autoFocus={index === 0} aria-label={action.ariaLabel ?? action.label} disabled={action.disabled} onKeyDown={stopActivation} onClick={event => { event.stopPropagation(); action.onClick(); }}>{action.label}</Button>)}
        {secondaryActions.map(action => <Button key={action.ariaLabel ?? action.label} variant="ghost" aria-label={action.ariaLabel ?? action.label} disabled={action.disabled} onKeyDown={stopActivation} onClick={event => { event.stopPropagation(); action.onClick(); }}>{action.label}</Button>)}
      </div>
      {view.details && <details onClick={event => event.stopPropagation()} onKeyDown={stopActivation} className="col-span-2 row-start-3 w-full min-w-0 text-xs text-[var(--ct-muted-fg)]">
        <summary className="cursor-pointer">Details</summary>
        <pre tabIndex={0} className="mt-3 max-h-48 overflow-auto whitespace-pre-wrap break-words text-left font-mono [overflow-wrap:anywhere]">{view.details}</pre>
        {detailActions.map(action => <Button key={action.ariaLabel ?? action.label} size="sm" aria-label={action.ariaLabel ?? action.label} disabled={action.disabled} onClick={event => { event.stopPropagation(); action.onClick(); }}>{action.label}</Button>)}
        <Button size="sm" onClick={() => { void copyDetails(); }}>Copy</Button>
      </details>}
      {path && <Button variant="icon" size="sm" className="col-start-2 row-start-1" aria-label="Copy folder path" title="Copy folder path" onClick={() => { void copyPath(); }}><Copy size={14} aria-hidden="true" /></Button>}
      {copyStatus && <p role="status" className="col-span-2 row-start-4 text-xs text-[var(--ct-muted-fg)]">{copyStatus}</p>}
    </div>
  </BrandState>;
}
