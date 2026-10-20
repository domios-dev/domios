import { useState } from "react";
import { Check } from "lucide-react";
import { Button } from "../shared/Button";
import { Chip } from "../shared/Chip";
import { LiquidToggle } from "../shared/LiquidToggle";
import {
  bestYearlySavings,
  offerIdForPlan,
  offerPrice,
  type BillingInterval,
  type PlanCatalog,
  type PlanOffer,
} from "./planCatalog";
import { usePlanCatalog } from "./planCatalogContext";

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

export interface PlanCardProps {
  offer: PlanOffer;
  interval: BillingInterval;
  current?: boolean;
  canPurchase?: boolean;
  onChoose?: (planKey: string) => void;
}

export function PlanCard({ offer, interval, current = false, canPurchase = true, onChoose }: PlanCardProps) {
  const price = offerPrice(offer, interval);
  const border = offer.recommended ? "border-[var(--ct-accent)]" : "border-[var(--ct-border-subtle)]";
  return (
    <section
      aria-label={`${offer.name} plan`}
      className={`flex flex-col gap-3 rounded-[var(--ct-radius-md)] border ${border} bg-[var(--ct-bg)] p-4`}
    >
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
      {price.planKey && !current && onChoose && (
        <Button
          size="sm"
          variant={offer.recommended ? "primary" : "ghost"}
          className="text-xs"
          disabled={!canPurchase}
          onClick={() => price.planKey && onChoose(price.planKey)}
        >
          Choose {offer.name}
        </Button>
      )}
    </section>
  );
}

/** Interval switch plus one card per offer; defaults to yearly billing and the catalog in context. */
export function PlanCards({ currentPlan = null, canPurchase = true, onChoose, catalog, initialInterval = "year" }: {
  currentPlan?: string | null;
  canPurchase?: boolean;
  onChoose?: (planKey: string) => void;
  catalog?: PlanCatalog;
  initialInterval?: BillingInterval;
}) {
  const contextCatalog = usePlanCatalog();
  const offers = catalog ?? contextCatalog;
  const [interval, setInterval] = useState<BillingInterval>(initialInterval);
  const currentOffer = offerIdForPlan(offers, currentPlan);
  return (
    <div className="flex flex-col gap-3">
      <BillingIntervalToggle interval={interval} onChange={setInterval} savings={bestYearlySavings(offers)} />
      <div className="grid gap-3 [grid-template-columns:repeat(auto-fit,minmax(200px,1fr))]">
        {offers.offers.map((offer) => (
          <PlanCard
            key={offer.id}
            offer={offer}
            interval={interval}
            current={offer.id === currentOffer}
            canPurchase={canPurchase}
            onChoose={onChoose}
          />
        ))}
      </div>
    </div>
  );
}
