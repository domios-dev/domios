import { useId, type CSSProperties } from "react";
import { CircleAlert } from "lucide-react";
import { RuntimeLoginActions } from "./RuntimeLoginActions";
import { runtimeStatusPresentation, type AgentRuntimeState } from "../../lib/agentRuntimeState";
import { TabLoadingIndicator } from "../shared/TabLoadingIndicator";
import { useRuntimeStatusPopover } from "./useRuntimeStatusPopover";

export function AgentRuntimeBanner({ state, onRetry, tabId, iconSvg }: { tabId?: string; state: AgentRuntimeState | null; onRetry: () => void; iconSvg?: string | null }) {
  if (tabId && state?.auth === "signedOut" && state.loginMethods?.length) {
    return <RuntimeLoginActions key={`${tabId}:${state.epoch}`} tabId={tabId} state={state} iconSvg={iconSvg} onCheck={onRetry} />;
  }
  const view = runtimeStatusPresentation(state);
  return view ? <RuntimeStatusBadge view={view} onRetry={onRetry} /> : null;
}

function RuntimeStatusBadge({ view, onRetry }: { view: NonNullable<ReturnType<typeof runtimeStatusPresentation>>; onRetry: () => void }) {
  const { open, triggerRef, popoverRef, toggle } = useRuntimeStatusPopover();
  const id = useId();
  const transient = view.severity === "transient";
  return <div className="relative z-40 h-0">
    <div className="absolute bottom-0 left-0 mb-2">
      <button ref={triggerRef} type="button" aria-label={view.label} aria-expanded={open} aria-haspopup="dialog" aria-controls={open ? id : undefined}
        onClick={toggle} className={`flex h-6 w-6 items-center justify-center rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] transition-colors hover:bg-[var(--ct-border)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-border-focus)] ${transient ? "text-[var(--ct-muted-fg)]" : "text-[var(--ct-accent)]"}`}>
        {transient ? <TabLoadingIndicator label={view.label} compact deferred={false} role="img" /> : <CircleAlert size={14} aria-hidden="true" />}
      </button>
      {open && <div id={id} ref={popoverRef} role="dialog" aria-label={view.label} tabIndex={-1}
        className="ct-menu absolute bottom-full left-0 mb-2 w-64 max-w-[calc(100vw-2rem)] p-3 text-xs text-[var(--ct-fg)]"
        style={{ "--ct-menu-origin": "bottom left", "--ct-menu-shift": "6px" } as CSSProperties}>
        <p role="status" aria-live="polite">{view.reason}</p>
        {view.details && <details className="mt-2 text-[var(--ct-muted-fg)]">
          <summary className="cursor-pointer">Details</summary>
          <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words font-mono">{view.details}</pre>
        </details>}
        <button type="button" aria-label="Retry" onClick={onRetry} disabled={!view.canRetry}
          className="mt-2 rounded-md border border-[var(--ct-border)] px-2.5 py-1 text-xs font-medium transition-colors hover:bg-[var(--ct-border)] disabled:cursor-default disabled:opacity-50 disabled:hover:bg-transparent">Retry</button>
      </div>}
    </div>
  </div>;
}
