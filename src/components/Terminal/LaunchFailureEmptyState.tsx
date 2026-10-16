import { useId, useState } from "react";
import { Folder, Slash, TriangleAlert, Copy } from "lucide-react";
import { Button } from "../shared/Button";
import { describeError } from "../../lib/describeError";
import type { ErrorNoticeAction } from "../shared/ErrorNotice";

export function LaunchFailureEmptyState({ reason, path, missingFolder, onChooseFolder, onClose, busy, actions }: {
  reason: string | null;
  path?: string | null;
  missingFolder: boolean;
  onChooseFolder?: () => void;
  onClose?: () => void;
  busy?: boolean;
  actions: ErrorNoticeAction[];
}) {
  const headingId = useId();
  const [copyStatus, setCopyStatus] = useState("");
  const view = describeError(reason, { operation: "starting this tab", cwd: path });
  const copyPath = async () => {
    try { await navigator.clipboard.writeText(path ?? ""); setCopyStatus("Path copied"); }
    catch { setCopyStatus("Could not copy the path"); }
  };
  return <section role="alert" aria-labelledby={headingId} className="grid w-full min-w-0 max-w-md grid-cols-[minmax(0,1fr)_auto] items-center gap-3 text-center text-[var(--ct-fg)]">
    <span aria-hidden="true" className="relative col-span-2 justify-self-center text-[var(--ct-muted-fg)]">{missingFolder ? <><Folder size={28} /><Slash size={28} className="absolute inset-0" /></> : <TriangleAlert size={28} />}</span>
    <h2 id={headingId} tabIndex={-1} className="col-span-2 text-sm font-semibold">{missingFolder ? "Folder unavailable" : view.title}</h2>
    <p className="col-span-2 text-sm text-[var(--ct-muted-fg)]">{missingFolder ? "Choose a folder to start this tab again." : view.cause}</p>
    {path && <div title={path} className="col-start-1 row-start-4 flex max-w-full min-w-0 rounded-md bg-[var(--ct-surface)] px-3 py-2 font-mono text-xs text-[var(--ct-muted-fg)]">
      <span className="min-w-0 truncate">{path.slice(0, Math.ceil(path.length / 2))}</span><span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap [direction:rtl]">{path.slice(Math.ceil(path.length / 2))}</span>
    </div>}
    <div className="col-span-2 row-start-5 flex flex-wrap items-center justify-center gap-2">
      {onChooseFolder && <Button variant="primary" autoFocus disabled={busy} onClick={onChooseFolder}>Choose folder</Button>}
      {!onChooseFolder && actions.map((action, index) => <Button key={action.ariaLabel ?? action.label} variant={index === 0 ? "primary" : "ghost"} autoFocus={index === 0} aria-label={action.ariaLabel ?? action.label} disabled={action.disabled} onClick={action.onClick}>{action.label}</Button>)}
      {onClose && <Button variant="ghost" onClick={onClose}>Close tab</Button>}
    </div>
    {view.raw && <details className="col-span-2 row-start-6 w-full min-w-0 text-xs text-[var(--ct-muted-fg)]">
      <summary className="cursor-pointer">Details</summary>
      <pre tabIndex={0} className="mt-3 max-h-48 overflow-auto whitespace-pre-wrap break-words text-left font-mono [overflow-wrap:anywhere]">{view.raw}</pre>
      {onChooseFolder && actions.map(action => <Button key={action.ariaLabel ?? action.label} size="sm" aria-label={action.ariaLabel ?? action.label} disabled={action.disabled} onClick={action.onClick}>{action.label}</Button>)}
    </details>}
    {path && <Button variant="icon" size="sm" className="col-start-2 row-start-4" aria-label="Copy folder path" title="Copy folder path" onClick={() => { void copyPath(); }}><Copy size={14} aria-hidden="true" /></Button>}
    {copyStatus && <p role="status" className="col-span-2 text-xs text-[var(--ct-muted-fg)]">{copyStatus}</p>}
  </section>;
}
