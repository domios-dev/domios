import { InlineErrorNotice } from "../shared/ErrorNotice";
import { describeError } from "../../lib/describeError";
import { useId, type CSSProperties } from "react";
import { CircleAlert } from "lucide-react";
import { RuntimeLoginActions } from "./RuntimeLoginActions";
import { runtimeStatusPresentation, type AgentRuntimeState } from "../../lib/agentRuntimeState";
import { TabLoadingIndicator } from "../shared/TabLoadingIndicator";
import { useRuntimeStatusPopover } from "./useRuntimeStatusPopover";
import { useComposerIssues, type ActionableComposerIssue } from "./ComposerIssuesContext";

export function AgentRuntimeBanner({ state, onRetry, tabId, iconSvg }: { tabId?: string; state: AgentRuntimeState | null; onRetry: () => void; iconSvg?: string | null }) {
  const issues = useComposerIssues();
  if (tabId && state?.auth === "signedOut" && state.loginMethods?.length) {
    return <>
      {issues.length > 0 && <RuntimeStatusBadge view={null} issues={issues} onRetry={onRetry} />}
      <RuntimeLoginActions key={`${tabId}:${state.epoch}`} tabId={tabId} state={state} iconSvg={iconSvg} onCheck={onRetry} />
    </>;
  }
  const view = runtimeStatusPresentation(state);
  return view || issues.length ? <RuntimeStatusBadge view={view} issues={issues} onRetry={onRetry} /> : null;
}

function RuntimeStatusBadge({ view, issues, onRetry }: { view: ReturnType<typeof runtimeStatusPresentation>; issues: ActionableComposerIssue[]; onRetry: () => void }) {
  const { open, triggerRef, popoverRef, toggle } = useRuntimeStatusPopover();
  const id = useId();
  const transient = !issues.length && view?.severity === "transient";
  const label = issues.length ? `${issues.length} attachment${issues.length === 1 ? "" : "s"} failed` : view!.label;
  return <div className="relative z-40 h-0">
    <div className="absolute bottom-0 left-0 mb-2">
      <button ref={triggerRef} type="button" aria-label={label} aria-expanded={open} aria-haspopup="dialog" aria-controls={open ? id : undefined}
        onClick={toggle} className={`flex h-6 w-6 items-center justify-center rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] transition-colors hover:bg-[var(--ct-border)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-border-focus)] ${issues.length ? "text-[var(--ct-warn)]" : transient ? "text-[var(--ct-muted-fg)]" : "text-[var(--ct-accent)]"}`}>
        {transient ? <TabLoadingIndicator label={label} compact deferred={false} role="img" /> : <CircleAlert size={14} aria-hidden="true" />}
      </button>
      {open && <div id={id} ref={popoverRef} role="dialog" aria-label={label} tabIndex={-1}
        className="ct-menu absolute bottom-full left-0 mb-2 w-64 max-w-[calc(100vw-2rem)] p-3 text-xs text-[var(--ct-fg)]"
        style={{ "--ct-menu-origin": "bottom left", "--ct-menu-shift": "6px" } as CSSProperties}>
        {issues.length > 0 && <ul aria-label="Attachment issues" className="max-h-64 space-y-3 overflow-auto">
          {issues.map((issue, index) => <li key={index}>
            <InlineErrorNotice description={{ ...describeError({ code: issue.kind === "too-large" ? "attachment_too_large" : "attachment_upload_failed", raw: issue.detail }), title: issue.title, cause: issue.explanation ?? `The attachment ${issue.name} could not be sent. Choose an action to continue.` }}
              actions={issue.actions.map(action => {
                const label = action === "retry" ? "Retry" : action === "remove" ? "Remove" : "Send as file path";
                return { label, ariaLabel: `${label} attachment ${issue.name}`, onClick: () => issue.run(action) };
              })} />
          </li>)}
        </ul>}
        {view && <InlineErrorNotice
          description={view.severity === "transient" ? { title: view.label, cause: view.reason, raw: view.details ?? "", actions: [] } : undefined}
          error={view.details} operation="connecting to the agent runtime"
          actions={[{ label: "Retry", disabled: !view.canRetry, onClick: onRetry }]} />}
      </div>}
    </div>
  </div>;
}
