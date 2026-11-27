import { useEffect, useId, useMemo, useRef, useState, type FormEvent } from "react";
import { ArrowRight, Check, X } from "lucide-react";
import { cloudPaywallShownEvent } from "../../lib/cloudMachines/analytics";
import { CloudApiError, createCloudPackCheckout } from "../../lib/cloudMachines/client";
import { recordProductEvent } from "../../lib/productAnalytics";
import {
  evaluateTopUpSelection,
  nextTierUpgradeLink,
  proComparisonStrip,
  resolveTopUpCatalog,
  type TopUpMode,
} from "../../lib/cloudMachines/topUpModel";
import type { CloudPackCheckoutReply, CloudUsagePayload } from "../../lib/cloudMachines/types";
import { nativeBridge } from "../../lib/nativeBridge";
import { DotMatrixCanvas, type DotMatrixCanvasHandle } from "../brand/DotMatrixCanvas";
import { usePrefersReducedMotion } from "../brand/usePrefersReducedMotion";
import { Button } from "../shared/Button";
import { Chip } from "../shared/Chip";
import { CommunityKeysNotice } from "../shared/CommunityLinks";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { FormDialog } from "../shared/FormDialog";
import { MachineGlyph } from "../shared/MachineGlyph";
import { NumericInput } from "../shared/NumericInput";
import { tierName } from "./desktopBilling";
import { usePlanCatalog } from "./planCatalogContext";
import { usePurchasesEnabled } from "./purchaseSwitch";
import "../MeshPanel/cloudMachines.css";

export interface PendingCloudTopUpCheckout {
  mode: TopUpMode;
  machineId?: string;
  machineName?: string;
  size?: string;
  packId: string;
  amountCents: number;
  expectedHours: number | null;
  priorRemaining: number;
  rolloverSentence: string | null;
}

export interface CloudTopUpDialogProps {
  mode: TopUpMode;
  usage: CloudUsagePayload | null;
  currentPlan?: string | null;
  machineId?: string;
  machineName?: string;
  size?: string;
  purchases?: boolean;
  onClose: () => void;
  onCheckoutStarted?: (pending: PendingCloudTopUpCheckout, reply: CloudPackCheckoutReply) => void;
  onOpenPlans?: () => void;
  startCheckout?: typeof createCloudPackCheckout;
  openUrl?: (url: string) => Promise<void> | void;
}

export function CloudTopUpDialog({
  mode,
  usage,
  currentPlan,
  machineId,
  machineName,
  size,
  purchases: explicitPurchases,
  onClose,
  onCheckoutStarted,
  onOpenPlans,
  startCheckout = createCloudPackCheckout,
  openUrl = (url) => nativeBridge.openUrl(url),
}: CloudTopUpDialogProps) {
  const titleId = useId();
  const descriptionId = useId();
  const customErrorId = useId();
  const remotePurchases = usePurchasesEnabled();
  const purchases = explicitPurchases ?? remotePurchases;
  const planCatalog = usePlanCatalog();
  const catalog = useMemo(() => resolveTopUpCatalog(usage), [usage]);
  const defaultPackId = useMemo(
    () => catalog.packs.find((pack) => pack.recommended)?.id ?? catalog.packs[0]?.id ?? "custom",
    [catalog.packs],
  );
  const [selection, setSelection] = useState<string>(defaultPackId);
  const [customRaw, setCustomRaw] = useState("");
  const [customValue, setCustomValue] = useState<number | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const customInputRef = useRef<HTMLDivElement>(null);
  const dotsRef = useRef<DotMatrixCanvasHandle>(null);
  const reportedPaywallModeRef = useRef<TopUpMode | null>(null);
  const reducedMotion = usePrefersReducedMotion();

  useEffect(() => {
    if (reportedPaywallModeRef.current === mode) return;
    reportedPaywallModeRef.current = mode;
    void recordProductEvent(cloudPaywallShownEvent(mode));
  }, [mode]);

  useEffect(() => {
    if (selection !== "custom" && !catalog.packs.some((pack) => pack.id === selection)) {
      setSelection(defaultPackId);
    }
  }, [catalog.packs, defaultPackId, selection]);

  useEffect(() => {
    if (selection !== "custom") return;
    customInputRef.current?.querySelector<HTMLInputElement>("input")?.focus();
  }, [selection]);

  const evaluation = useMemo(
    () =>
      evaluateTopUpSelection({
        selection,
        customRaw,
        catalog,
        currentRemaining: usage?.remaining ?? 0,
        mode,
        machineName,
      }),
    [selection, customRaw, catalog, usage?.remaining, mode, machineName],
  );

  const openPlans = () => {
    if (onOpenPlans) {
      onOpenPlans();
      return;
    }
    onClose();
    window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:plans" } }));
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!purchases || submitting || evaluation.ctaDisabled || !evaluation.checkoutPayload) return;
    setSubmitting(true);
    setSubmitError(null);
    try {
      const reply = await startCheckout(evaluation.checkoutPayload);
      await openUrl(reply.url);
      onCheckoutStarted?.(
        {
          mode,
          machineId,
          machineName,
          size,
          packId: reply.pack_id,
          amountCents: reply.amount_cents,
          expectedHours: evaluation.hours ?? reply.credits,
          priorRemaining: usage?.remaining ?? 0,
          rolloverSentence: catalog.rolloverSentence,
        },
        reply,
      );
      onClose();
    } catch (err) {
      if (err instanceof CloudApiError) {
        switch (err.code) {
          case "spend_cap_exceeded":
            setSubmitError("This purchase would exceed your monthly cloud spend limit.");
            break;
          case "below_min":
          case "above_max":
            setSubmitError(err.message);
            break;
          case "territory":
            setSubmitError("Cloud hour purchases are not available in your region.");
            break;
          case "rate_limited":
            setSubmitError(
              err.retryAfterSeconds
                ? `Too many checkout attempts. Try again in ${err.retryAfterSeconds}s.`
                : "Too many checkout attempts. Try again in a moment.",
            );
            break;
          case "local_owner_only":
            setSubmitError("Open Domios directly on this computer to buy cloud hours.");
            break;
          case "unavailable":
            setSubmitError("Checkout is temporarily unavailable. Try again in a moment.");
            break;
          case "plan_required":
            setSubmitError("Upgrade your plan to buy cloud hours.");
            break;
          default:
            setSubmitError(err.message);
            break;
        }
      } else {
        setSubmitError("Couldn't start checkout. Try again.");
      }
    } finally {
      setSubmitting(false);
    }
  };

  const title =
    mode === "wake"
      ? `Top up to wake ${machineName || "machine"}`
      : mode === "free"
        ? "Start your cloud machine"
        : "Top up cloud hours";

  const subtitle =
    mode === "wake"
      ? `Your files and held message are waiting. ${machineName || "Your machine"} wakes as soon as hours arrive.`
      : mode === "free"
        ? "Billed only while active. Sleeps automatically when idle and keeps your files."
        : "Keeps your machines running while your computers sleep — billed only while active.";

  const trustItems = useMemo(() => {
    const list: string[] = [mode === "free" ? "One-time, no subscription" : "One-time payment"];
    if (catalog.rolloverChip) list.push(catalog.rolloverChip);
    if (mode === "topup" && currentPlan && currentPlan !== "free") {
      list.push(`${tierName(currentPlan)} hours spend first`);
    } else {
      list.push("No surprise overages");
    }
    return list;
  }, [catalog.rolloverChip, currentPlan, mode]);

  const proBanner = mode === "free" ? proComparisonStrip(planCatalog, catalog.allowanceHours) : null;
  const footerLink = mode !== "free" ? nextTierUpgradeLink(planCatalog, currentPlan, catalog.allowanceHours) : null;
  const isCustom = selection === "custom";
  const showBalanceRow = mode !== "free";
  const currencySymbol = catalog.currency === "USD" ? "$" : `${catalog.currency} `;

  return (
    <FormDialog
      titleId={titleId}
      descriptionId={descriptionId}
      className="max-w-[524px] p-[22px] rounded-[14px]"
      onCancel={onClose}
      onSubmit={(event) => void handleSubmit(event)}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-start gap-2.5">
          <MachineGlyph kind="vm" size="tile" />
          <div className="min-w-0">
            <h2 id={titleId} className="m-0 text-base font-semibold tracking-[-0.015em] leading-[1.25] text-[var(--ct-fg)]">
              {title}
            </h2>
            <p id={descriptionId} className="m-0 mt-1 text-xs leading-[1.45] text-[var(--ct-muted-fg)] [text-wrap:balance]">
              {subtitle}
            </p>
          </div>
        </div>
        <button
          type="button"
          aria-label="Close"
          onClick={onClose}
          className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md border-0 bg-transparent text-[var(--ct-muted-fg)] cursor-pointer hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
        >
          <X size={16} aria-hidden="true" />
        </button>
      </div>

      {!purchases ? (
        <div className="mt-1 flex flex-col gap-3">
          <CommunityKeysNotice />
          <div className="flex justify-end">
            <Button type="button" size="sm" onClick={onClose}>
              Close
            </Button>
          </div>
        </div>
      ) : (
        <>
          <div role="radiogroup" aria-label="Cloud credit pack" className="mt-1 flex flex-col gap-2">
            {catalog.packs.length > 0 && (
              <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-3">
                {catalog.packs.map((pack) => {
                  const selected = selection === pack.id;
                  return (
                    <button
                      key={pack.id}
                      type="button"
                      role="radio"
                      aria-checked={selected}
                      data-selected={selected ? "true" : undefined}
                      onClick={() => {
                        setSelection(pack.id);
                        setSubmitError(null);
                      }}
                      onPointerEnter={pack.recommended && selected ? () => dotsRef.current?.emitRelease(performance.now()) : undefined}
                      className="ct-card ct-card--choice relative isolate overflow-hidden p-3 text-left cursor-pointer"
                    >
                      {pack.recommended && selected && (
                        <div className="pointer-events-none absolute -right-10 -top-12 -z-10 h-28 w-40">
                          <DotMatrixCanvas
                            ref={dotsRef}
                            mode="loading"
                            reducedMotion={reducedMotion}
                            intensity={3}
                            restIntensity={8}
                            waveDurationMs={1000}
                          />
                        </div>
                      )}
                      <div className="flex items-center justify-between gap-1.5">
                        <span className="text-[21px] font-semibold leading-[1.1] tracking-[-0.02em] tabular-nums text-[var(--ct-fg)]">
                          {pack.amountLabel}
                        </span>
                        {pack.recommended ? (
                          <Chip variant="accent" className={selected ? "" : "ct-cloud-chip-soft"}>
                            Recommended
                          </Chip>
                        ) : (
                          <span aria-hidden="true" className="ct-cloud-radio" data-selected={selected ? "true" : undefined} />
                        )}
                      </div>
                      <div className="mt-2.5 text-[14px] font-semibold tabular-nums text-[var(--ct-fg)]">
                        {pack.hoursLabel}
                      </div>
                      <div className="mt-0.5 text-[11.5px] text-[var(--ct-muted-fg)]">{pack.realEstimate}</div>
                    </button>
                  );
                })}
              </div>
            )}

            <div
              role="radio"
              tabIndex={0}
              aria-checked={isCustom}
              aria-label="Custom amount"
              data-selected={isCustom ? "true" : undefined}
              onClick={() => {
                setSelection("custom");
                setSubmitError(null);
              }}
              onKeyDown={(event) => {
                if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) {
                  event.preventDefault();
                  setSelection("custom");
                }
              }}
              className="ct-card ct-card--choice px-3 py-2.5 text-left cursor-pointer"
            >
              <div className="flex min-h-7 items-center gap-2.5">
                <span aria-hidden="true" className="ct-cloud-radio" data-selected={isCustom ? "true" : undefined} />
                <span className="text-[13px] font-medium text-[var(--ct-fg)]">Custom amount</span>
                {isCustom ? (
                  <div className="ml-auto flex items-center gap-2.5" onClick={(event) => event.stopPropagation()}>
                    <div
                      ref={customInputRef}
                      className="ct-cloud-input-shell w-[90px]"
                      data-invalid={evaluation.error ? "true" : undefined}
                    >
                      <span aria-hidden="true" className="text-xs text-[var(--ct-muted-fg)]">
                        {currencySymbol}
                      </span>
                      <NumericInput
                        aria-label="Custom amount in whole dollars"
                        aria-invalid={evaluation.error ? "true" : undefined}
                        aria-describedby={evaluation.error ? customErrorId : undefined}
                        integer
                        value={customValue}
                        onValueChange={(next) => {
                          setCustomValue(next);
                          setCustomRaw(String(next));
                          setSubmitError(null);
                        }}
                        onEmpty={() => {
                          setCustomValue(null);
                          setCustomRaw("");
                          setSubmitError(null);
                        }}
                        className="w-full min-w-0 border-0 bg-transparent p-0 text-xs font-medium tabular-nums text-[var(--ct-fg)] outline-none"
                      />
                    </div>
                    <div className="w-[128px] shrink-0 text-right leading-[1.25]">
                      {evaluation.valid && evaluation.hours !== null ? (
                        <>
                          <div className="text-[12.5px] font-semibold tabular-nums text-[var(--ct-fg)]">
                            {Number.isInteger(evaluation.hours) ? evaluation.hours : evaluation.hours.toFixed(1)} hours
                          </div>
                          {evaluation.realEstimate && (
                            <div className="text-[11px] text-[var(--ct-muted-fg)]">{evaluation.realEstimate}</div>
                          )}
                        </>
                      ) : (
                        <div className="h-7" aria-hidden="true" />
                      )}
                    </div>
                  </div>
                ) : (
                  catalog.unitRateLabel && (
                    <span className="ml-auto text-[11.5px] tabular-nums text-[var(--ct-muted-fg)]">
                      {catalog.unitRateLabel}
                    </span>
                  )
                )}
              </div>
            </div>
          </div>

          {evaluation.error && (
            <div id={customErrorId}>
              <InlineErrorNotice role="alert" description={{ title: evaluation.error, cause: "", actions: [], raw: "" }} />
            </div>
          )}
          {submitError && (
            <InlineErrorNotice role="alert" description={{ title: submitError, cause: "", actions: [], raw: "" }} />
          )}

          <div className="rounded-[10px] border border-[var(--ct-border-subtle)] bg-[color-mix(in_srgb,var(--ct-fg)_2%,var(--ct-bg))] px-3 py-2.5">
            {showBalanceRow && (
              <>
                <div className="flex items-center justify-between gap-2.5">
                  <span className="text-xs text-[var(--ct-muted-fg)]">
                    {evaluation.afterBalanceLabel ? "Balance after top-up" : "Current balance"}
                  </span>
                  <span className="inline-flex items-center gap-1.5 text-[12.5px] tabular-nums" data-testid="topup-balance-preview">
                    {evaluation.afterBalanceLabel ? (
                      <>
                        <span className="text-[var(--ct-muted-fg)]">{evaluation.currentBalanceLabel}</span>
                        <ArrowRight size={12} aria-hidden="true" className="text-[var(--ct-muted-fg)]" />
                        <strong className="font-semibold text-[var(--ct-fg)]">{evaluation.afterBalanceLabel}</strong>
                      </>
                    ) : (
                      <strong className="font-semibold text-[var(--ct-fg)]">{evaluation.currentBalanceLabel}</strong>
                    )}
                  </span>
                </div>
                <div className="my-2 h-px bg-[var(--ct-border-subtle)]" />
              </>
            )}
            <div className="flex flex-wrap items-center justify-between gap-2 text-[11.5px] text-[var(--ct-muted-fg)]">
              {trustItems.map((item) => (
                <span key={item} className="inline-flex items-center gap-1.5 whitespace-nowrap">
                  <Check size={12} strokeWidth={2.25} className="shrink-0 opacity-85" aria-hidden="true" />
                  <span>{item}</span>
                </span>
              ))}
            </div>
          </div>

          {proBanner && (
            <div className="ct-card ct-card--quiet flex items-center justify-between gap-3 rounded-[10px] border border-[var(--ct-border)] px-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[12.5px] font-semibold text-[var(--ct-fg)]">{proBanner.title}</div>
                {proBanner.subtitle && (
                  <div className="mt-0.5 text-[11.5px] text-[var(--ct-muted-fg)]">{proBanner.subtitle}</div>
                )}
              </div>
              <button
                type="button"
                onClick={openPlans}
                className="shrink-0 border-0 bg-transparent p-0 text-xs font-medium text-[var(--ct-accent)] cursor-pointer hover:underline"
              >
                {proBanner.actionLabel}
              </button>
            </div>
          )}

          <div className="mt-1 flex flex-wrap items-center gap-2">
            {footerLink ? (
              <button
                type="button"
                onClick={openPlans}
                className="mr-auto border-0 bg-transparent p-0 text-left text-xs text-[var(--ct-accent)] cursor-pointer hover:underline"
              >
                {footerLink}
              </button>
            ) : (
              <span className="mr-auto" />
            )}
            <Button type="button" size="sm" variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button
              type="submit"
              size="sm"
              variant="primary"
              disabled={evaluation.ctaDisabled || submitting}
              loading={submitting}
            >
              {evaluation.ctaLabel}
            </Button>
          </div>
        </>
      )}
    </FormDialog>
  );
}
