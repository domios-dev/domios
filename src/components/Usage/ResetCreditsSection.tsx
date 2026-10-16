import {useRef, useSyncExternalStore} from "react";
import {createPortal} from "react-dom";
import {formatUntil} from "../../lib/timeLabels";
import type {ResetCredits} from "@codeterm/plugin-sdk";
import {ConfirmDialog} from "../Extensions/ConfirmDialog";
import {InlineErrorNotice} from "../shared/ErrorNotice";
import {resetCreditsState, resetOutcomeLabels, type ResetCreditsModel} from "./resetCreditsModel";

export function ResetCreditsSection({resets, model}: {resets?:ResetCredits|null; model:ResetCreditsModel}) {
  const resetButton = useRef<HTMLButtonElement>(null);
  useSyncExternalStore(model.subscribe, model.snapshot, model.snapshot);
  const state = resetCreditsState(resets);
  if (!state.visible || !resets) return null;
  const run = (operation: () => Promise<void>) => {void operation();};
  const cancel = () => {model.cancel(); resetButton.current?.focus();};
  return <section aria-label="Rate limit resets" className="ct-usage-reset-credits">
    <div>{resets.availableCount} resets available</div>
    {resets.nextExpiry != null && <div>Next known expiry: {formatUntil(resets.nextExpiry, Date.now())} · <time dateTime={new Date(resets.nextExpiry).toISOString()}>{new Date(resets.nextExpiry).toLocaleString()}</time></div>}
    <button ref={resetButton} type="button" className="ct-btn" disabled={state.disabled || model.busy || !!model.key} onClick={() => {model.request(resets);}}>Reset now</button>
    {model.key && !model.busy && <button type="button" className="ct-btn" onClick={() => run(() => model.retry())}>Retry reset</button>}
    {model.busy && <div role="status">Resetting limits…</div>}
    {model.outcome && <div role="status">{resetOutcomeLabels[model.outcome]}</div>}
    {model.error && <InlineErrorNotice error={model.error} operation="resetting the rate limits" />}
    {model.confirming && createPortal(<div onMouseDown={event => event.stopPropagation()} ref={node => node?.querySelector<HTMLButtonElement>("button")?.focus()} onKeyDown={event => {
      if (event.key === "Escape") {event.stopPropagation();cancel();}
      if (event.key === "Tab") {
        const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button")];
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        event.preventDefault(); buttons[(index + (event.shiftKey ? buttons.length - 1 : 1)) % buttons.length]?.focus();
      }
    }}><ConfirmDialog title="Reset current rate limits?" message="This uses one reset credit to reset the current limit windows. It cannot be undone." confirmLabel="Confirm reset" onCancel={cancel} onConfirm={() => run(() => model.confirm())} /></div>, document.body)}
  </section>;
}
