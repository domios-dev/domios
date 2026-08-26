import { useId, useRef, useState } from "react";
import { Check, ChevronDown } from "lucide-react";

export interface DestinationOption {
  id: string;
  /** The caption the registry gave the channel. */
  label: string;
  /** This option's own accessible name, composed by the caller from the level
   *  it belongs to, so two levels never offer two identically named controls. */
  name: string;
  selected: boolean;
  onToggle: (selected: boolean) => void;
}

/**
 * One level's push destination, as a single control.
 *
 * The push side is a registry rather than a fixed pair of surfaces, so it gets
 * a picker instead of a control per member: a channel the registry gains widens
 * this list without adding a row to every level. The choice is genuinely a set —
 * none, one, or several — so the options are checkboxes a reader can enumerate
 * rather than a single-choice menu, and the trigger reads back the whole answer.
 */
export function DestinationPicker({
  name,
  label,
  summary,
  options,
}: {
  name: string;
  label: string;
  summary: string;
  options: readonly DestinationOption[];
}) {
  const [open, setOpen] = useState(false);
  const listId = useId();
  const triggerRef = useRef<HTMLButtonElement>(null);

  return (
    <div
      className="relative flex items-center justify-between gap-2"
      data-testid="settings-destination-picker"
      onKeyDown={(event) => {
        if (event.key !== "Escape" || !open) return;
        event.stopPropagation();
        setOpen(false);
        triggerRef.current?.focus();
      }}
      onBlur={(event) => {
        if (event.currentTarget.contains(event.relatedTarget as Node | null)) return;
        setOpen(false);
      }}
    >
      <span className="text-xs text-[var(--ct-muted-fg)]">{label}</span>
      <button
        ref={triggerRef}
        type="button"
        data-testid="settings-destination-picker-trigger"
        aria-label={name}
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        onClick={() => setOpen((current) => !current)}
        className="flex min-w-0 max-w-[60%] items-center gap-1.5 rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] px-2 py-1 text-[11px] text-[var(--ct-fg)] outline-none"
      >
        <span className="min-w-0 flex-1 truncate text-left">{summary}</span>
        <ChevronDown aria-hidden size={12} className="shrink-0 text-[var(--ct-muted-fg)]" />
      </button>
      {open && (
        <div
          id={listId}
          role="group"
          aria-label={name}
          data-testid="settings-destination-picker-options"
          className="absolute right-0 top-full z-10 mt-1 flex min-w-[180px] flex-col gap-0.5 rounded-md border border-[var(--ct-border)] bg-[var(--ct-surface)] p-1 shadow-lg"
        >
          {options.map((option) => (
            <button
              key={option.id}
              type="button"
              role="checkbox"
              aria-checked={option.selected}
              aria-label={option.name}
              data-destination={option.id}
              onClick={() => option.onToggle(!option.selected)}
              className="flex items-center gap-2 rounded-sm px-2 py-1 text-left text-[11px] text-[var(--ct-fg)] hover:bg-[var(--ct-hover)]"
            >
              <span className="flex h-3 w-3 shrink-0 items-center justify-center text-[var(--ct-accent)]">
                {option.selected && <Check aria-hidden size={11} />}
              </span>
              <span className="min-w-0 truncate">{option.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
