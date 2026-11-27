import { UI_BUTTON, UI_TEXT } from "../shared/design";
import {
  formatStaleMachineNames,
  type StaleMachineCandidate,
} from "./staleMachinesModel";

export interface StaleCleanupConfirmProps {
  machines: readonly StaleMachineCandidate[];
  busy: boolean;
  error?: string | null;
  onConfirm: () => Promise<void>;
  onCancel: () => void;
}

export function StaleCleanupConfirm({
  machines,
  busy,
  error,
  onConfirm,
  onCancel,
}: StaleCleanupConfirmProps) {
  if (machines.length === 0) return null;
  const { visible, overflow } = formatStaleMachineNames(machines);
  const count = machines.length;
  const noun = count === 1 ? "machine" : "machines";

  return (
    <div
      role="group"
      aria-label={`Remove ${count} stale ${noun}`}
      className="flex flex-col gap-2 rounded-md border border-[var(--ct-border)] bg-[var(--ct-surface)] px-3.5 py-2.5"
    >
      <p className={UI_TEXT.meta}>
        Remove {count} stale {noun} from your mesh? This machine and online machines are never affected.
      </p>
      <ul className="m-0 flex flex-wrap gap-x-2 gap-y-1 p-0 list-none text-[12px] text-[var(--ct-fg)]">
        {visible.map((name, index) => (
          <li
            key={`${name}-${index}`}
            className="rounded bg-[var(--ct-hover)] px-1.5 py-0.5 font-medium"
          >
            {name}
          </li>
        ))}
        {overflow > 0 && (
          <li className="px-1 py-0.5 text-[var(--ct-muted-fg)]">
            +{overflow} more
          </li>
        )}
      </ul>
      {error && (
        <p role="alert" className="m-0 text-[12px] text-[var(--ct-danger)]">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <button
          type="button"
          disabled={busy}
          aria-busy={busy}
          className={`${UI_BUTTON.danger} ${UI_BUTTON.compact}`}
          onClick={() => void onConfirm()}
        >
          {busy ? "Removing…" : `Remove ${count} ${noun}`}
        </button>
        <button
          type="button"
          disabled={busy}
          className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact}`}
          onClick={onCancel}
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
