import { useEffect, useRef, useState, type ReactNode, type Ref } from "react";
import { Check } from "lucide-react";
import { nativeBridge } from "../../lib/nativeBridge";
import { siteLinks } from "../../../shared/siteLinks";
import { ActivityDots } from "../shared/ActivityDots";
import { Button } from "../shared/Button";
import { Chip } from "../shared/Chip";
import { LiquidToggle } from "../shared/LiquidToggle";
import { SubscriptionTermsNotice } from "../shared/LegalLinks";
import { DotMatrixCanvas, type DotMatrixCanvasHandle } from "../brand/DotMatrixCanvas";
import { usePrefersReducedMotion } from "../brand/usePrefersReducedMotion";
import {
  bestYearlySavings,
  offerIdForPlan,
  offerPrice,
  planCardAction,
  type BillingInterval,
  type PlanCardAction,
  type PlanCatalog,
  type PlanOffer,
} from "./planCatalog";
import { usePlanCatalog } from "./planCatalogContext";
import { usePlanViewed } from "./usePlanViewed";
import { IDLE_INTENT, type PurchaseIntent } from "./purchaseIntent";

export function BillingIntervalToggle({ interval, onChange, savings }: {
  interval: BillingInterval;
  onChange: (interval: BillingInterval) => void;
  savings?: number | null;
}) {
  return (
    <label className="flex w-fit cursor-pointer items-center gap-2 text-xs text-[var(--ct-fg)]">
      <LiquidToggle
        aria-label="Billed yearly"
        checked={interval === "year"}
        onCheckedChange={(yearly) => onChange(yearly ? "year" : "month")}
      />
      <span>Billed yearly</span>
      {savings ? <Chip variant="success">Save up to {savings}%</Chip> : null}
    </label>
  );
}

export type PurchaseIntentView =
  | { kind: "signing_in" }
  | { kind: "opening" }
  | { kind: "owned"; staff: boolean; signedInAs: string | null };

const STATUS_TONES = {
  accent: "bg-[color-mix(in_srgb,var(--ct-accent)_9%,transparent)] text-[var(--ct-accent)]",
  green: "bg-[color-mix(in_srgb,var(--ct-green)_10%,transparent)] text-[var(--ct-green)]",
} as const;

function IntentStatus({ tone, icon, title, detail, focusRef }: { tone: keyof typeof STATUS_TONES; icon: ReactNode; title: string; detail: string | null; focusRef?: Ref<HTMLDivElement> }) {
  return (
    <div ref={focusRef} tabIndex={focusRef ? -1 : undefined} role="status" aria-live="polite" className={`flex items-start gap-2 rounded-[var(--ct-radius-sm)] px-2.5 py-2 ${STATUS_TONES[tone]}`}>
      {icon}
      <div className="flex min-w-0 flex-col">
        <strong className="text-xs font-medium text-[var(--ct-fg)]">{title}</strong>
        {detail && <small className="text-[11px] text-[var(--ct-muted-fg)]">{detail}</small>}
      </div>
    </div>
  );
}

function useIntentFocus(kind: PurchaseIntentView["kind"] | undefined) {
  const choose = useRef<HTMLButtonElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const status = useRef<HTMLDivElement>(null);
  const previous = useRef(kind);
  useEffect(() => {
    const was = previous.current;
    previous.current = kind;
    // The control that held focus unmounts on every transition, so focus moves to what replaces it.
    if (kind === "signing_in") cancel.current?.focus();
    else if (kind === "opening" || kind === "owned") status.current?.focus();
    else if (was === "signing_in" || was === "opening") choose.current?.focus();
  }, [kind]);
  return { choose, cancel, status };
}

export interface PlanCardProps {
  offer: PlanOffer;
  interval: BillingInterval;
  current?: boolean;
  canPurchase?: boolean;
  onChoose?: (planKey: string) => void;
  intent?: PurchaseIntentView;
  onCancel?: () => void;
  action?: PlanCardAction;
  onManage?: () => void;
  manageBusy?: boolean;
}

export function PlanCard({ offer, interval, current = false, canPurchase = true, onChoose, intent, onCancel, action: givenAction, onManage, manageBusy = false }: PlanCardProps) {
  const price = offerPrice(offer, interval);
  const action: PlanCardAction = givenAction ?? (price.planKey ? { kind: "choose", planKey: price.planKey } : { kind: "none" });
  const border = offer.recommended ? "border-[var(--ct-accent)]" : "border-[var(--ct-border-subtle)]";
  const focus = useIntentFocus(intent?.kind);
  const dots = useRef<DotMatrixCanvasHandle>(null);
  const reducedMotion = usePrefersReducedMotion();
  const wave = () => dots.current?.emitRelease(performance.now());
  useEffect(() => { if (offer.recommended) wave(); }, [offer.recommended]);
  return (
    <section
      aria-label={`${offer.name} plan`}
      onPointerEnter={offer.recommended ? wave : undefined}
      className={`flex flex-col gap-3 rounded-[var(--ct-radius-md)] border ${border} bg-[var(--ct-bg)] p-4${offer.recommended ? " relative isolate overflow-hidden" : ""}`}
    >
      {offer.recommended && <div className="pointer-events-none absolute -right-14 -top-16 -z-10 h-44 w-64">
        <DotMatrixCanvas ref={dots} mode="loading" reducedMotion={reducedMotion} intensity={3} restIntensity={8} waveDurationMs={1_000} />
      </div>}
      <header className="flex items-center justify-between gap-2">
        <h3 className="m-0 text-sm font-semibold text-[var(--ct-fg)]">{offer.name}</h3>
        {current ? <Chip variant="success">Current</Chip> : offer.recommended && <Chip variant="accent">Recommended</Chip>}
      </header>
      <p className="m-0 text-xs text-[var(--ct-muted-fg)]">{offer.tagline}</p>
      <div className="flex flex-col gap-0.5">
        <p className="m-0 flex items-baseline gap-1">
          <span className="text-3xl font-semibold tabular-nums text-[var(--ct-fg)]">{price.headline}</span>
          <span className="text-xs text-[var(--ct-muted-fg)]">{price.per}</span>
        </p>
        <p className="m-0 text-xs text-[var(--ct-muted-fg)]">{price.note}</p>
      </div>
      <ul className="m-0 flex flex-1 list-none flex-col gap-1.5 p-0">
        {offer.features.map((feature) => (
          <li key={feature} className="flex items-start gap-2 text-xs text-[var(--ct-fg)]">
            <Check size={14} className="mt-px shrink-0 text-[var(--ct-accent)]" aria-hidden="true" />
            {feature}
          </li>
        ))}
      </ul>
      {intent?.kind === "signing_in" ? (
        <div className="flex flex-col gap-2">
          <IntentStatus tone="accent" icon={<ActivityDots size="small" className="mt-1.5 shrink-0" />}
            title="Finish signing in in your browser." detail={`Checkout for ${offer.name} opens next.`} />
          <div className="flex justify-end">
            <Button ref={focus.cancel} size="sm" variant="ghost" className="text-xs" onClick={onCancel}>Cancel</Button>
          </div>
        </div>
      ) : intent?.kind === "opening" ? (
        <div ref={focus.status} tabIndex={-1} className="flex flex-col outline-none">
          <Button size="lg" variant="outline" loading>Opening checkout…</Button>
        </div>
      ) : intent?.kind === "owned" ? (
        <IntentStatus tone="green" focusRef={focus.status} icon={<Check size={14} className="mt-px shrink-0" aria-hidden="true" />}
          title={`You already have ${offer.name}.`}
          detail={intent.staff ? "Included with your Domios team access." : intent.signedInAs && `Signed in as ${intent.signedInAs}.`} />
      ) : action.kind === "choose" && !current && onChoose ? (
        <Button ref={focus.choose} size="lg" variant="outline" disabled={!canPurchase} onClick={() => onChoose(action.planKey)}>
          Choose {offer.name}
        </Button>
      ) : action.kind === "manage" && onManage ? (
        <div className="flex flex-col gap-2">
          <Button size="lg" variant="outline" disabled={manageBusy} loading={manageBusy} onClick={onManage}>Manage subscription</Button>
          <p className="m-0 text-xs text-[var(--ct-muted-fg)]">To switch plans, cancel in Manage subscription and choose a new plan after it ends, or contact us.</p>
        </div>
      ) : null}
    </section>
  );
}

function intentView(intent: PurchaseIntent, signedInAs: string | null): PurchaseIntentView | undefined {
  if (intent.kind === "idle") return undefined;
  if (intent.kind === "owned") return { kind: "owned", staff: intent.staff, signedInAs };
  return { kind: intent.kind };
}

/** One card per offer, then the interval switch beside the purchase terms; yearly by default. */
export function PlanCards({ currentPlan = null, subscribedPlan = null, onManage, manageBusy = false, canPurchase = true, onChoose, catalog, interval: controlledInterval, onIntervalChange, intent = IDLE_INTENT, signedInAs = null, onCancel, viewRequested = false }: {
  viewRequested?: boolean;
  currentPlan?: string | null;
  /** A live paid subscription; while set, other paid cards offer Manage subscription instead of a checkout. */
  subscribedPlan?: string | null;
  onManage?: () => void;
  manageBusy?: boolean;
  canPurchase?: boolean;
  onChoose?: (planKey: string) => void;
  catalog?: PlanCatalog;
  interval?: BillingInterval;
  onIntervalChange?: (interval: BillingInterval) => void;
  intent?: PurchaseIntent;
  signedInAs?: string | null;
  onCancel?: () => void;
}) {
  const contextCatalog = usePlanCatalog();
  const viewTarget = usePlanViewed(viewRequested);
  const offers = catalog ?? contextCatalog;
  const [ownInterval, setOwnInterval] = useState<BillingInterval>("year");
  const interval = controlledInterval ?? ownInterval;
  const changeInterval = onIntervalChange ?? setOwnInterval;
  const currentOffer = offerIdForPlan(offers, currentPlan);
  const intentOffer = intent.kind === "idle" ? null : offerIdForPlan(offers, intent.planKey);
  const view = intentView(intent, signedInAs);
  return (
    <div ref={viewTarget} className="flex flex-col gap-3">
      <div role="group" aria-label="Subscription plans" className="grid gap-3 [grid-template-columns:repeat(auto-fit,minmax(200px,1fr))]">
        {offers.offers.map((offer) => (
          <PlanCard
            key={offer.id}
            offer={offer}
            interval={interval}
            current={offer.id === currentOffer}
            canPurchase={canPurchase}
            onChoose={onChoose}
            intent={offer.id === intentOffer ? view : undefined}
            onCancel={onCancel}
            action={planCardAction(offers, offer, interval, subscribedPlan)}
            onManage={onManage}
            manageBusy={manageBusy}
          />
        ))}
      </div>
      <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
        <BillingIntervalToggle interval={interval} onChange={changeInterval} savings={bestYearlySavings(offers)} />
        <SubscriptionTermsNotice />
      </div>
      <section aria-label="Enterprise plan" className="flex w-full items-center justify-between gap-3 rounded-[var(--ct-radius-md)] border border-[var(--ct-border-subtle)] bg-[var(--ct-bg)] p-4">
        <div className="flex min-w-0 flex-col gap-1">
          <h3 className="m-0 text-sm font-semibold text-[var(--ct-fg)]">Enterprise</h3>
          <p className="m-0 text-xs text-[var(--ct-muted-fg)]">Running Domios at scale for a big team? Contact us.</p>
        </div>
        <Button size="sm" variant="ghost" className="shrink-0 text-xs" onClick={() => void nativeBridge.openUrl(siteLinks.discord)}>
          Contact us
        </Button>
      </section>
    </div>
  );
}
