import { useMemo, useState } from "react";
import { Plus, X } from "lucide-react";
import {
  cloudSizeOptions,
  formatCreditHours,
  sizeMultiplierHint,
} from "../../lib/cloudMachines/stateModel";
import type { CloudUsagePayload } from "../../lib/cloudMachines/types";
import { LockBadge } from "../billing/LockBadge";
import { Button } from "../shared/Button";
import { Segmented } from "../shared/Segmented";
import { MACHINE_CARD_WIDTH } from "./webGraphLayout";
import "./cloudMachines.css";

export interface CloudMachineSlotProps {
  usage: CloudUsagePayload | null;
  isFree: boolean;
  onCreateCloud: (size: string) => void;
  onPairComputer: () => void;
  onCancel: () => void;
}

export function CloudMachineSlot({
  usage,
  isFree,
  onCreateCloud,
  onPairComputer,
  onCancel,
}: CloudMachineSlotProps) {
  const [kind, setKind] = useState<"cloud" | "computer">("cloud");
  const sizes = useMemo(() => cloudSizeOptions(usage), [usage]);
  const [selectedSize, setSelectedSize] = useState<string>(() => sizes[0]?.id ?? "small");
  const activeSize = sizes.find((option) => option.id === selectedSize) ?? sizes[0];
  const smallOption = sizes.find((option) => option.id === "small") ?? sizes[0];
  const remaining = usage?.remaining ?? 0;
  const freeLocked = isFree && remaining <= 0;

  const cloudSubline = freeLocked && smallOption?.rateLabel
    ? `${smallOption.rateLabel} ${smallOption.label} · runs while your computers sleep`
    : "Keeps working while your computers sleep";

  const sizeHint = activeSize ? sizeMultiplierHint(activeSize, sizes, freeLocked) : null;
  const footerStatus =
    freeLocked || (usage !== null && remaining <= 0)
      ? "No cloud hours yet"
      : usage !== null
        ? `${formatCreditHours(remaining)} of cloud time left`
        : "Managed cloud machine";

  const handlePrimary = () => {
    if (kind === "computer") {
      onPairComputer();
      return;
    }
    onCreateCloud(activeSize?.id ?? "small");
  };

  return (
    <section
      aria-label="New machine"
      data-testid="cloud-new-machine-slot"
      onClick={(event) => event.stopPropagation()}
      className="ct-card ct-card--static ct-cloud-slot flex flex-col text-left"
      style={{ width: MACHINE_CARD_WIDTH }}
    >
      <div className="ct-card__head">
        <span
          aria-hidden="true"
          className="ct-card__icon shrink-0"
          style={{ ["--ct-card-tone" as string]: "var(--ct-muted-fg)" }}
        >
          <Plus size={15} strokeWidth={2} />
        </span>
        <span className="min-w-0 flex-1 truncate text-[14px] font-medium text-[var(--ct-fg)]">
          New machine
        </span>
        <button
          type="button"
          aria-label="Close new machine"
          onClick={onCancel}
          className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md border-0 bg-transparent text-[var(--ct-muted-fg)] cursor-pointer hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
        >
          <X size={14} aria-hidden="true" />
        </button>
      </div>

      <div role="radiogroup" aria-label="Machine type" className="grid gap-1 px-2 pb-2 pt-0.5">
        <button
          type="button"
          role="radio"
          aria-checked={kind === "cloud"}
          aria-selected={kind === "cloud"}
          onClick={() => setKind("cloud")}
          className="ct-machine-option min-h-[48px]"
          style={{ ["--ct-switcher-tone" as string]: "var(--ct-accent)" }}
        >
          <span aria-hidden="true" className="ct-cloud-radio" data-selected={kind === "cloud" ? "true" : undefined} />
          <span className="ct-machine-option__copy">
            <span className="ct-machine-option__name">
              <span>Cloud machine</span>
              {freeLocked && <LockBadge view={{ locked: true, label: "Cloud machines require hours or a paid plan" }} />}
            </span>
            <span className="ct-machine-option__status">{cloudSubline}</span>
          </span>
        </button>

        <button
          type="button"
          role="radio"
          aria-checked={kind === "computer"}
          aria-selected={kind === "computer"}
          onClick={() => setKind("computer")}
          className="ct-machine-option min-h-[46px]"
          style={{ ["--ct-switcher-tone" as string]: "var(--ct-accent)" }}
        >
          <span aria-hidden="true" className="ct-cloud-radio" data-selected={kind === "computer" ? "true" : undefined} />
          <span className="ct-machine-option__copy">
            <span className="ct-machine-option__name">One of your computers</span>
            <span className="ct-machine-option__status">Install Domios on it and pair</span>
          </span>
        </button>
      </div>

      {kind === "cloud" && (
        <div className="flex flex-wrap items-center gap-2.5 px-3.5 pb-3 pt-1">
          {sizes.length > 1 ? (
            <Segmented
              aria-label="Cloud machine size"
              value={activeSize?.id ?? "small"}
              options={sizes.map((option) => ({ value: option.id, label: option.label }))}
              onChange={setSelectedSize}
            />
          ) : (
            <span className="inline-flex h-6 items-center rounded-md bg-[color-mix(in_srgb,var(--ct-fg)_6%,transparent)] px-2.5 text-xs font-medium text-[var(--ct-fg)]">
              {activeSize?.label ?? "Small"}
            </span>
          )}
          {sizeHint && (
            <span className="text-[11px] text-[var(--ct-muted-fg)]">{sizeHint}</span>
          )}
        </div>
      )}

      <div
        className="flex min-h-10 items-center gap-2 border-t px-3.5 pr-2.5 text-[11px] text-[var(--ct-muted-fg)]"
        style={{ borderColor: "color-mix(in srgb, var(--ct-border) 55%, transparent)" }}
      >
        <span className="min-w-0 flex-1 truncate tabular-nums">{footerStatus}</span>
        <div className="flex shrink-0 items-center gap-1.5">
          <Button type="button" size="sm" variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="button" size="sm" variant="primary" onClick={handlePrimary}>
            {kind === "computer" ? "Continue" : "Create machine"}
          </Button>
        </div>
      </div>
    </section>
  );
}
