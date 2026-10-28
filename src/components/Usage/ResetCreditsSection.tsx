import {creditExpiryHint} from "./creditExpiry";
import {useRef, useSyncExternalStore} from "react";
import {createPortal} from "react-dom";
import {formatUntil} from "../../lib/timeLabels";
import type {ResetCredit, ResetCredits} from "@codeterm/plugin-sdk";
import {ConfirmDialog} from "../Extensions/ConfirmDialog";
import {InlineErrorNotice} from "../shared/ErrorNotice";
import {availableResetCredits, resetCreditLabel, resetCreditsState, resetOutcomeLabels, resetBalanceLabel, type ResetCreditsModel} from "./resetCreditsModel";

function expiryLabel(expiresAt: number | null | undefined) {
  if (expiresAt == null) return "Expiry not reported";
  const hint = creditExpiryHint(expiresAt, Date.now());
  if (hint) return <span className="ct-usage-credit-expiry">{hint}</span>;
  return <>Expires <time dateTime={new Date(expiresAt).toISOString()} title={new Date(expiresAt).toLocaleString()}>{formatUntil(expiresAt, Date.now())}</time></>;
}

const resetIcon = <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="var(--ct-accent, #6366f1)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
  <path d="M3 12a9 9 0 1 0 3-6.7" /><path d="M3 4v5h5" /><path d="M12 8v4l3 2" />
</svg>;

export function ResetCreditsSection({resets, model}: {resets?:ResetCredits|null; model:ResetCreditsModel}) {
  const lastUse = useRef<HTMLButtonElement | null>(null);
  useSyncExternalStore(model.subscribe, model.snapshot, model.snapshot);
  const state = resetCreditsState(resets, model.selectedCreditId);
  const credits = availableResetCredits(resets);
  if (!state.visible || !resets) return null;
  const locked = model.busy || !!model.key || model.confirming || resets.availableCount === 0;
  const use = (creditId: string | null, button: HTMLButtonElement) => {
    lastUse.current = button;
    model.select(resets, creditId);
    model.request(resets);
  };
  const cancel = () => {model.cancel(); lastUse.current?.focus();};
  const rows: {id: string | null; credit: ResetCredit | null}[] = credits.length
    ? credits.map(credit => ({id: credit.id, credit}))
    : resets.availableCount > 0 ? [{id: null, credit: null}] : [];
  const pending = model.pendingCredit;
  const remaining = model.beforeCount != null ? Math.max(model.beforeCount - 1, 0) : null;
  return <section aria-label="Session resets" className="ct-usage-reset-credits">
    <div className="ct-usage-reset-heading">
      <span className="ct-usage-reset-label">Session resets</span>
      <span className="ct-usage-reset-count" aria-label={`${resets.availableCount} available`}>{resets.availableCount}</span>
    </div>
    {rows.length > 0 && <ul className="ct-usage-reset-list">
      {rows.map(({id, credit}, index) => {
        const base = credit ? resetCreditLabel(credit) : "Session reset";
        const name = rows.filter(row => (row.credit ? resetCreditLabel(row.credit) : "Session reset") === base).length > 1 ? `${base} ${index + 1}` : base;
        return <li key={id ?? "next"} className="ct-usage-reset-row" title={credit?.description}>
          <span className="ct-usage-reset-text">
            <span className="ct-usage-reset-name">{name}</span>
            <span className="ct-usage-reset-note">{credit ? expiryLabel(credit.expiresAt) : resets.nextExpiry != null ? expiryLabel(resets.nextExpiry) : "Chosen by provider"}</span>
          </span>
          <button type="button" className="ct-usage-reset-use" aria-label={`Use ${name}`} disabled={locked || state.disabled} onClick={event => use(id, event.currentTarget)}>Use</button>
        </li>;
      })}
    </ul>}
    {credits.length > 0 && credits.length < resets.availableCount && <div className="ct-usage-reset-note">Details for {credits.length} of {resets.availableCount} resets reported.</div>}
    {model.key && !model.busy && <button type="button" className="ct-usage-reset-use" onClick={() => {void model.retry();}}>Retry reset</button>}
    {model.busy && <div role="status" className="ct-usage-reset-note">Resetting limits…</div>}
    {model.outcome && <div role="status" className="ct-usage-reset-note">{resetOutcomeLabels[model.outcome]} · {resetBalanceLabel(model.beforeCount, model.afterCount)}</div>}
    {model.error && <InlineErrorNotice error={model.error} operation="resetting the rate limits" />}
    {model.confirming && createPortal(<div data-anchored-popover="" onMouseDown={event => event.stopPropagation()} ref={node => node?.querySelector<HTMLButtonElement>("button")?.focus()} onKeyDown={event => {
      if (event.key === "Escape") {event.stopPropagation();cancel();}
      if (event.key === "Tab") {
        const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button")];
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        event.preventDefault(); buttons[(index + (event.shiftKey ? buttons.length - 1 : 1)) % buttons.length]?.focus();
      }
    }}><ConfirmDialog
      icon={resetIcon}
      title="Use a session reset?"
      message={<>
        <strong style={{color: "var(--ct-fg)", fontWeight: 600}}>{pending ? resetCreditLabel(pending) : "The next available reset"}</strong>
        {pending?.expiresAt != null && <> · {expiryLabel(pending.expiresAt)}</>}
        {" "}resets your current limit windows right away.
        {remaining != null && <> {remaining} of {model.beforeCount} will remain.</>}
        {" "}This can’t be undone.
      </>}
      confirmLabel="Use reset"
      onCancel={cancel}
      onConfirm={() => {void model.confirm();}} /></div>, document.body)}
  </section>;
}
