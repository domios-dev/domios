import { hasRowControls, RuntimeDeclaredControls } from "./RuntimeDeclaredControls";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useId } from "react";
import { CircleAlert } from "lucide-react";
import { runtimeSignInRequired } from "./RuntimeLoginActions";
import { runtimeStatusPresentation, type AgentRuntimeState } from "../../lib/agentRuntimeState";
import { TabLoadingIndicator } from "../shared/TabLoadingIndicator";
import { useRuntimeStatusPopover } from "./useRuntimeStatusPopover";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { describeRuntimeConnectionError, isMissingRuntimeConversation } from "../../lib/describeError";
import { useStartNewSession } from "./useStartNewSession";

export function AgentRuntimeBanner({ state, onRetry, tabId, startupOwned = false }: { tabId?: string; state: AgentRuntimeState | null; onRetry: () => void; iconSvg?: string | null; startupOwned?: boolean }) {
  const hasControls = hasRowControls(state);
  // Sign-in has its own card in the chat area (TabSessionChat), never inside the composer group.
  if (tabId && state && runtimeSignInRequired(state)) {
    return <>{hasControls && <RuntimeDeclaredControls tabId={tabId} state={state} parts="notice" />}</>;
  }
  const view = runtimeStatusPresentation(state, startupOwned);
  return <>
    {tabId && state && hasControls && <RuntimeDeclaredControls tabId={tabId} state={state} parts="notice" />}
    <RuntimeStatusBadge view={view} onRetry={onRetry} tabId={tabId} />
  </>;
}

function RuntimeStatusBadge({ view, onRetry, tabId }: { view: ReturnType<typeof runtimeStatusPresentation>; onRetry: () => void; tabId?: string }) {
  const fresh = useStartNewSession(tabId ?? "");
  const missing = isMissingRuntimeConversation(view?.details);
  const { open, triggerRef, popoverRef, toggle, close } = useRuntimeStatusPopover();
  const id = useId();
  const active = Boolean(view);
  const transient = view?.severity === "transient";
  const label = view?.label ?? "Composer status";
  return <div className="ct-composer-issue relative z-40 h-0" data-active={active} aria-hidden={!active} inert={!active}>
    <div className="absolute bottom-0 left-0 mb-2">
      <button ref={triggerRef} type="button" disabled={!active} aria-label={label} aria-expanded={active && open} aria-haspopup={active ? "dialog" : undefined} aria-controls={active && open ? id : undefined}
        onClick={toggle} className={`flex h-6 w-6 items-center justify-center rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] transition-colors hover:bg-[var(--ct-border)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-border-focus)] ${transient ? "text-[var(--ct-muted-fg)]" : "text-[var(--ct-accent)]"}`}>
        {transient ? <TabLoadingIndicator label={label} compact deferred={false} role="img" /> : <CircleAlert size={14} aria-hidden="true" />}
      </button>
      <AnchoredPopover open={active && open} anchor={triggerRef} placement="top-start" onClose={close}>
      <div id={id} ref={popoverRef} role="dialog" aria-label={label} tabIndex={-1}
        className="ct-menu w-64 max-w-full p-3 text-xs text-[var(--ct-fg)]">
        {view && <InlineErrorNotice
          description={view.severity === "transient" ? { title: view.label, cause: view.reason, raw: view.details ?? "", actions: [] } : missing ? undefined : describeRuntimeConnectionError(view.details)}
          error={view.details} operation="connecting to the agent runtime"
          actions={missing && tabId
            ? [{label:"Start a new conversation",disabled:fresh.startingNewSession,onClick:() => void fresh.onStartNewSession()}]
            : [{ label: "Retry", disabled: !view.canRetry, onClick: onRetry }]} />}
        {fresh.actionError && <InlineErrorNotice error={fresh.actionError} operation="starting a new conversation" />}
      </div>
      </AnchoredPopover>
    </div>
  </div>;
}
