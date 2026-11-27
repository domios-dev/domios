import { useEffect, useId, useRef } from "react";
import { Check } from "lucide-react";
import { formatCreditHours } from "../../lib/cloudMachines/stateModel";
import { BusyState } from "../shared/BusyState";
import { Button } from "../shared/Button";
import { FormDialog } from "../shared/FormDialog";

export interface CloudTopUpReturnDialogProps {
  addedHours: number;
  newBalance: number;
  rolloverSentence: string | null;
  wakeTarget?: { machineName: string; onOpen: () => void } | null;
  creatingMachine?: boolean;
  onClose: () => void;
}

export function CloudTopUpReturnDialog({
  addedHours,
  newBalance,
  rolloverSentence,
  wakeTarget,
  creatingMachine = false,
  onClose,
}: CloudTopUpReturnDialogProps) {
  const titleId = useId();
  const descriptionId = useId();
  const primaryRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    primaryRef.current?.focus();
  }, []);

  const hoursNumber = Number.isInteger(addedHours) ? String(addedHours) : addedHours.toFixed(1);

  return (
    <FormDialog
      titleId={titleId}
      descriptionId={descriptionId}
      className="max-w-[420px] p-[22px] rounded-[14px]"
      onCancel={onClose}
      onSubmit={(event) => {
        event.preventDefault();
        if (wakeTarget) wakeTarget.onOpen();
        else onClose();
      }}
    >
      <span
        aria-hidden="true"
        className="grid h-[34px] w-[34px] place-items-center rounded-full bg-[color-mix(in_srgb,var(--ct-green)_13%,transparent)] text-[var(--ct-green)]"
      >
        <Check size={18} strokeWidth={2.25} />
      </span>

      <h2 id={titleId} className="m-0 text-base font-semibold tracking-[-0.015em] text-[var(--ct-fg)]">
        {hoursNumber} cloud hours added
      </h2>

      <p id={descriptionId} className="m-0 text-xs leading-[1.5] text-[var(--ct-muted-fg)]">
        Your balance is now <strong className="font-semibold text-[var(--ct-fg)]">{formatCreditHours(newBalance)}</strong>.
        {rolloverSentence ? ` ${rolloverSentence}` : ""}
      </p>

      {wakeTarget && (
        <div
          role="status"
          aria-live="polite"
          className="mt-1 flex items-start gap-2.5 rounded-[10px] bg-[color-mix(in_srgb,var(--ct-accent)_9%,transparent)] px-3 py-2.5"
        >
          <span className="mt-1 shrink-0">
            <BusyState inline hideLabel phase="work" dotSize="micro" label={`Waking ${wakeTarget.machineName}…`} />
          </span>
          <div className="min-w-0">
            <div className="text-[12.5px] font-semibold text-[var(--ct-fg)]">Waking {wakeTarget.machineName}…</div>
            <div className="mt-0.5 text-[11.5px] text-[var(--ct-muted-fg)]">
              Your held message will deliver as soon as it&apos;s ready.
            </div>
          </div>
        </div>
      )}

      {!wakeTarget && creatingMachine && (
        <div
          role="status"
          aria-live="polite"
          className="mt-1 flex items-start gap-2.5 rounded-[10px] bg-[color-mix(in_srgb,var(--ct-accent)_9%,transparent)] px-3 py-2.5"
        >
          <span className="mt-1 shrink-0">
            <BusyState inline hideLabel phase="work" dotSize="micro" label="Preparing your machine…" />
          </span>
          <div className="min-w-0">
            <div className="text-[12.5px] font-semibold text-[var(--ct-fg)]">Preparing your machine…</div>
          </div>
        </div>
      )}

      <div className="mt-2 flex justify-end gap-2">
        {wakeTarget ? (
          <>
            <Button type="button" size="sm" variant="ghost" onClick={onClose}>
              Close
            </Button>
            <Button ref={primaryRef} type="submit" size="sm" variant="primary">
              Open {wakeTarget.machineName}
            </Button>
          </>
        ) : (
          <Button ref={primaryRef} type="submit" size="sm" variant="primary">
            Done
          </Button>
        )}
      </div>
    </FormDialog>
  );
}
