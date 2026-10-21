import type {UsageDetails} from "../../types/usage";
import {creditExpiryHint} from "./creditExpiry";
import {nativeBridge} from "../../lib/nativeBridge";
import {UsageGauge} from "./UsageGauge";
import {classify} from "../../types/usage";

export function UsageScopedLimits({details, now}: {details?: UsageDetails | null; now: number}) {
  return <>{details?.limits.filter(limit => limit.kind === "weekly_scoped").map((limit, index) => (
    <UsageGauge key={index} label={`${limit.label}${limit.is_active ? " · active" : ""}`} variant="weekly" pct={limit.percent} resetsAt={limit.resets_at} state={limit.critical ? "danger" : classify(limit.percent)} now={now} />
  ))}</>;
}

export function UsageAllowanceRows({details, now}: {details?: UsageDetails | null; now: number}) {
  if (!details) return null;
  const extra = details.extra_usage;
  const dollars = (amount: number) => new Intl.NumberFormat("en-US", {style: "currency", currency: "USD", maximumFractionDigits: 2}).format(amount).replace(/\.00$/, "");
  return <>
    {details.dollar_credits.length > 0 && <section className="ct-usage-reset-credits" aria-label="Usage credit allowance">
      <ul className="ct-usage-reset-list">{details.dollar_credits.map((credit, index) => {
        const hint = credit.remaining_dollars > 0 ? creditExpiryHint(credit.resets_at, now) : null;
        return <li className="ct-usage-reset-row" key={index}>
          <span className="ct-usage-reset-text">
            <span className="ct-usage-reset-name">{dollars(credit.remaining_dollars)} credit left{credit.resets_at != null && <> · until <time dateTime={new Date(credit.resets_at).toISOString()}>{new Date(credit.resets_at).toLocaleDateString("en-US", {month: "short", day: "numeric"})}</time></>}</span>
            {hint && <span className="ct-usage-reset-note ct-usage-credit-expiry">{hint}</span>}
          </span>
        </li>;
      })}</ul>
    </section>}
    {extra && <div className="ct-usage-account-credits">
      <span className="k">Usage credits {extra.enabled ? "on" : "off"}{extra.enabled && <> · {extra.used ?? "—"}/{extra.limit ?? "—"} {extra.currency}{extra.spend_limit_reached ? " · limit reached" : ""}</>}</span>
      <a href={extra.settings_url} target="_blank" rel="noopener noreferrer" onClick={event => {event.preventDefault(); void nativeBridge.openUrl(extra.settings_url);}}>Settings</a>
    </div>}
  </>;
}
