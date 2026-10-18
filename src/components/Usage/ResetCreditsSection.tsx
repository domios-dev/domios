import {useId, useRef, useSyncExternalStore} from "react";
import {createPortal} from "react-dom";
import {formatUntil} from "../../lib/timeLabels";
import type {ResetCredits} from "@codeterm/plugin-sdk";
import {ConfirmDialog} from "../Extensions/ConfirmDialog";
import {InlineErrorNotice} from "../shared/ErrorNotice";
import {availableResetCredits, resetCreditLabel, resetCreditsState, resetOutcomeLabels, resetBalanceLabel, type ResetCreditsModel} from "./resetCreditsModel";

export function ResetCreditsSection({resets, model}: {resets?:ResetCredits|null; model:ResetCreditsModel}) {
  const choiceName = useId();
  const resetButton = useRef<HTMLButtonElement>(null);
  useSyncExternalStore(model.subscribe, model.snapshot, model.snapshot);
  const state = resetCreditsState(resets, model.selectedCreditId);
  const credits = availableResetCredits(resets);
  if (!state.visible || !resets) return null;
  const run = (operation: () => Promise<void>) => {void operation();};
  const cancel = () => {model.cancel(); resetButton.current?.focus();};
  return <section aria-label="Rate limit resets" className="ct-usage-reset-credits">
    <div className="ct-usage-reset-heading"><span>Reset credits</span><span>{resets.availableCount} available</span></div>
    <fieldset aria-label="Reset credit to use" className="ct-usage-reset-choices" disabled={model.busy || !!model.key || model.confirming || resets.availableCount === 0}>
      <label className="ct-usage-reset-choice">
        <input type="radio" name={choiceName} checked={model.selectedCreditId === null} onChange={() => model.select(resets, null)} />
        <span>Next available <span className="ct-usage-reset-note">· chosen by provider</span></span>
      </label>
      {credits.map(credit => <label key={credit.id} className="ct-usage-reset-choice">
        <input type="radio" aria-label={`${resetCreditLabel(credit)}${credit.title ? ` · ${credit.id}` : ""}`} name={choiceName} value={credit.id} checked={model.selectedCreditId === credit.id} onChange={() => model.select(resets, credit.id)} />
        <span className="ct-usage-reset-entry" title={credit.description}>
          <span className="ct-usage-reset-title"><span title={credit.id}>{resetCreditLabel(credit)}</span>
            <span className="ct-usage-reset-note">{credit.expiresAt != null ? <>Expires <time dateTime={new Date(credit.expiresAt).toISOString()} title={new Date(credit.expiresAt).toLocaleString()}>{formatUntil(credit.expiresAt, Date.now())}</time></> : "Expiry not reported"}</span>
          </span>
          {credit.title && <span className="ct-usage-reset-note ct-usage-reset-id" title={credit.id}>{credit.id}</span>}
        </span>
      </label>)}
    </fieldset>
    {credits.length < resets.availableCount && <div className="ct-usage-reset-note">Details for {credits.length} of {resets.availableCount} credits reported.</div>}
    {!credits.length && resets.nextExpiry != null && <div className="ct-usage-reset-note">Next known expiry: <time dateTime={new Date(resets.nextExpiry).toISOString()}>{formatUntil(resets.nextExpiry, Date.now())}</time></div>}
    <button ref={resetButton} type="button" className="ct-btn" disabled={state.disabled || model.busy || !!model.key} onClick={() => {model.request(resets);}}>Reset now</button>
    {model.key && !model.busy && <button type="button" className="ct-btn" onClick={() => run(() => model.retry())}>Retry reset</button>}
    {model.busy && <div role="status">Resetting limits…</div>}
    {model.outcome && <div role="status">{resetOutcomeLabels[model.outcome]} · {resetBalanceLabel(model.beforeCount, model.afterCount)}</div>}
    {model.error && <InlineErrorNotice error={model.error} operation="resetting the rate limits" />}
    {model.confirming && createPortal(<div onMouseDown={event => event.stopPropagation()} ref={node => node?.querySelector<HTMLButtonElement>("button")?.focus()} onKeyDown={event => {
      if (event.key === "Escape") {event.stopPropagation();cancel();}
      if (event.key === "Tab") {
        const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button")];
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        event.preventDefault(); buttons[(index + (event.shiftKey ? buttons.length - 1 : 1)) % buttons.length]?.focus();
      }
    }}><ConfirmDialog title="Reset current rate limits?" message={`This spends ${model.pendingCredit ? `“${resetCreditLabel(model.pendingCredit)}” (${model.pendingCredit.id})` : "the next available reset credit chosen by the provider"} from the ${model.beforeCount} available to reset eligible limit windows. It cannot be undone.`} confirmLabel="Confirm reset" onCancel={cancel} onConfirm={() => run(() => model.confirm())} /></div>, document.body)}
  </section>;
}
