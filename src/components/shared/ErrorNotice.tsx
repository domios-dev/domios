import { useState } from "react";
import { describeError, type ErrorContext, type ErrorDescription } from "../../lib/describeError";

export interface ErrorNoticeAction { label: string; onClick: () => void; disabled?: boolean; ariaLabel?: string }
export interface ErrorNoticeProps extends ErrorContext {
  error?: unknown;
  description?: ErrorDescription;
  actions?: ErrorNoticeAction[];
  blocking?: boolean;
  compact?: boolean;
}

export function ErrorNotice({ error, description, actions = [], blocking = false, compact = false, ...context }: ErrorNoticeProps) {
  const view = description ?? describeError(error, context);
  const [copyState, setCopyState] = useState("");
  const copy = async () => {
    try { await navigator.clipboard.writeText(view.raw); setCopyState("Copied"); }
    catch { setCopyState("Copy was unavailable. Select the details and copy them manually."); }
  };
  return <section role={blocking ? "alert" : "status"} className={`${compact ? "text-xs" : "rounded-md border border-[var(--ct-border)] bg-[var(--ct-surface)] p-3 text-sm"} min-w-0 text-left text-[var(--ct-fg)]`}>
    <p className="font-medium">{view.title}</p>
    <p className="mt-1 break-words text-[var(--ct-muted-fg)]">{view.cause}</p>
    {actions.length > 0 && <div className="mt-2 flex flex-wrap gap-2">{actions.map(action =>
      <button key={action.ariaLabel ?? action.label} type="button" aria-label={action.ariaLabel ?? action.label} disabled={action.disabled} onClick={action.onClick}
        className="rounded-md border border-[var(--ct-border)] px-2 py-1 text-xs hover:bg-[var(--ct-bg-hover)] disabled:opacity-50">{action.label}</button>)}</div>}
    {view.raw && <details className="mt-2 text-xs text-[var(--ct-muted-fg)]">
      <summary className="cursor-pointer">Details</summary>
      <pre tabIndex={0} className="mt-2 max-h-60 select-text overflow-auto whitespace-pre-wrap break-words font-mono [overflow-wrap:anywhere]">{view.raw}</pre>
      <button type="button" onClick={() => { void copy(); }} className="mt-2 rounded border border-[var(--ct-border)] px-2 py-1">Copy</button>
      {copyState && <p role="status">{copyState}</p>}
    </details>}
  </section>;
}

export function InlineErrorNotice(props: Omit<ErrorNoticeProps, "compact">) {
  return <ErrorNotice {...props} compact />;
}
