import { useEffect, useId, useRef, useState } from "react";
import { Check } from "lucide-react";
import { CloudApiError } from "../../lib/cloudMachines/client";
import {
  normalizePromoCodeInput,
  promoRedeemOutcome,
  promoRefusalMessage,
} from "../../lib/cloudMachines/promoModel";
import { cloudUsageMeterView } from "../../lib/cloudMachines/usageModel";
import type { CloudPromoRedemption, CloudUsagePayload } from "../../lib/cloudMachines/types";
import { Button } from "../shared/Button";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import "./cloudMachines.css";

interface CloudUsageMeterProps {
  usage: CloudUsagePayload;
  planId?: string | null;
  onTopUp: () => void;
  onRedeem?: (code: string) => Promise<CloudPromoRedemption>;
  showRedeem?: boolean;
  className?: string;
}

export function CloudUsageMeter({
  usage,
  planId,
  onTopUp,
  onRedeem,
  showRedeem = true,
  className = "",
}: CloudUsageMeterProps) {
  const view = cloudUsageMeterView(usage, { planId });
  const [redeemOpen, setRedeemOpen] = useState(false);
  const [code, setCode] = useState("");
  const [redeeming, setRedeeming] = useState(false);
  const [redeemError, setRedeemError] = useState<string | null>(null);
  const [redeemSuccess, setRedeemSuccess] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const errorId = useId();

  useEffect(() => {
    if (redeemOpen) inputRef.current?.focus();
  }, [redeemOpen]);

  const toggleRedeem = () => {
    setRedeemOpen((open) => {
      const next = !open;
      if (!next) setRedeemError(null);
      else setRedeemSuccess(null);
      return next;
    });
  };

  const submitRedeem = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!onRedeem || redeeming) return;
    const normalized = normalizePromoCodeInput(code);
    if (!normalized) return;
    setRedeeming(true);
    setRedeemError(null);
    setRedeemSuccess(null);
    try {
      const reply = await onRedeem(normalized);
      const outcome = promoRedeemOutcome(reply, normalized);
      if (outcome.kind === "success") {
        setCode("");
        setRedeemOpen(false);
        setRedeemSuccess(outcome.message);
      } else {
        setRedeemError(outcome.message);
      }
    } catch (err) {
      const reason = err instanceof CloudApiError ? err.code : undefined;
      setRedeemError(promoRefusalMessage(reason));
    } finally {
      setRedeeming(false);
    }
  };

  const showCalmPrompt = view.lowBalance || view.exhausted;

  return (
    <section aria-label="Cloud hours" data-testid="cloud-usage-meter" className={`flex w-full flex-col ${className}`}>
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-[13px] font-semibold text-[var(--ct-fg)]">Cloud hours</span>
        {view.resetLabel && (
          <span className="text-[11px] text-[var(--ct-muted-fg)]">{view.resetLabel}</span>
        )}
      </div>

      <div className="mt-2 flex items-baseline gap-1.5">
        <span className="text-[24px] font-semibold leading-[1.1] tracking-[-0.02em] tabular-nums text-[var(--ct-fg)]" data-testid="cloud-usage-remaining">
          {view.remainingLabel}
        </span>
        <span className="text-[13px] text-[var(--ct-muted-fg)]">{view.contextLabel}</span>
      </div>

      <div
        className="ct-cloud-bar mt-2.5"
        role="progressbar"
        aria-label="Cloud hours remaining"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(view.monthlyPct + view.purchasedPct)}
      >
        {view.monthlyPct > 0 && (
          <div className="ct-cloud-bar__monthly" style={{ width: `${view.monthlyPct}%` }} />
        )}
        {view.purchasedPct > 0 && (
          <div className="ct-cloud-bar__purchased" style={{ width: `${view.purchasedPct}%` }} />
        )}
      </div>

      {view.showBuckets && (
        <div className="mt-2 flex items-center justify-between gap-2 text-[11.5px] tabular-nums text-[var(--ct-muted-fg)]">
          <span className="inline-flex items-center gap-1.5">
            <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full bg-[var(--ct-accent)]" />
            {view.monthlyBucketLabel}
          </span>
          <span className="inline-flex items-center gap-1.5">
            <span
              aria-hidden="true"
              className="h-1.5 w-1.5 rounded-full"
              style={{ background: "color-mix(in srgb, var(--ct-accent) 44%, transparent)" }}
            />
            {view.purchasedBucketLabel}
          </span>
        </div>
      )}

      {showCalmPrompt ? (
        <>
          <div className="mt-3 flex items-center justify-between gap-2.5 rounded-[10px] border border-[var(--ct-border-subtle)] bg-[color-mix(in_srgb,var(--ct-fg)_2%,var(--ct-bg))] px-3 py-2.5">
            <div className="min-w-0">
              <div className="text-[12px] font-semibold text-[var(--ct-fg)]">
                {view.exhausted ? "Out of credits: top up to wake" : "Running low on hours"}
              </div>
              <div className="mt-0.5 text-[11px] text-[var(--ct-muted-fg)]">
                {view.exhausted
                  ? "Your files stay safe while asleep."
                  : "Sleeps cleanly at 0 h and keeps work"}
              </div>
            </div>
            <Button type="button" size="sm" variant="primary" onClick={onTopUp}>
              Top up
            </Button>
          </div>
          {showRedeem && onRedeem && (
            <div className="mt-2 flex items-center justify-start">
              <button
                type="button"
                onClick={toggleRedeem}
                aria-expanded={redeemOpen}
                className={`border-0 bg-transparent p-0 text-[12px] cursor-pointer hover:underline ${
                  redeemOpen ? "text-[var(--ct-fg)]" : "text-[var(--ct-muted-fg)]"
                }`}
              >
                Redeem code
              </button>
            </div>
          )}
        </>
      ) : (
        <div className="mt-3 flex items-center justify-between gap-2">
          {showRedeem && onRedeem ? (
            <button
              type="button"
              onClick={toggleRedeem}
              aria-expanded={redeemOpen}
              className={`border-0 bg-transparent p-0 text-[12px] cursor-pointer hover:underline ${
                redeemOpen ? "text-[var(--ct-fg)]" : "text-[var(--ct-muted-fg)]"
              }`}
            >
              Redeem code
            </button>
          ) : (
            <span />
          )}
          <button
            type="button"
            onClick={onTopUp}
            className="border-0 bg-transparent p-0 text-[12px] font-medium text-[var(--ct-accent)] cursor-pointer hover:underline"
          >
            Top up
          </button>
        </div>
      )}

      {showRedeem && onRedeem && redeemOpen && (
        <form className="mt-2.5 flex flex-col gap-1.5" onSubmit={(event) => void submitRedeem(event)}>
          <div className="flex items-center gap-1.5">
            <input
              ref={inputRef}
              type="text"
              value={code}
              onChange={(event) => {
                setCode(event.target.value);
                if (redeemError) setRedeemError(null);
              }}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  event.preventDefault();
                  event.stopPropagation();
                  setRedeemOpen(false);
                  setRedeemError(null);
                }
              }}
              autoComplete="off"
              spellCheck={false}
              aria-label="Promo code"
              aria-invalid={redeemError ? "true" : undefined}
              aria-describedby={redeemError ? errorId : undefined}
              placeholder="Enter promo code"
              disabled={redeeming}
              className="ct-field min-w-0 flex-1 font-mono text-[11.5px]"
            />
            <Button
              type="submit"
              size="sm"
              variant="primary"
              disabled={redeeming || !code.trim()}
              loading={redeeming}
            >
              {redeeming ? "Redeeming…" : "Redeem"}
            </Button>
          </div>
          {redeemError && (
            <div id={errorId}>
              <InlineErrorNotice role="alert" description={{ title: redeemError }} />
            </div>
          )}
        </form>
      )}

      {redeemSuccess && (
        <p role="status" className="m-0 mt-2.5 flex items-center gap-1.5 text-[12px] leading-[1.4] text-[var(--ct-fg)]">
          <Check size={14} strokeWidth={2.25} className="shrink-0 text-[var(--ct-green)]" aria-hidden="true" />
          <span>{redeemSuccess}</span>
        </p>
      )}
    </section>
  );
}
