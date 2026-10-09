import { Clock3 } from "lucide-react";

const btnBase =
  "ct-liquid-interactive flex items-center justify-center shrink-0";

export function ComposerScheduleButton({ onSchedule }: { onSchedule: () => void }) {
  return (
    <button
      type="button"
      data-testid="composer-schedule-button"
      title="Schedule send"
      aria-label="Schedule send"
      onMouseDown={(e) => e.preventDefault()}
      onClick={(e) => {
        e.preventDefault();
        onSchedule();
      }}
      className={`${btnBase} ct-composer-round-button ct-composer-inner-button ct-composer-inner-control rounded-full border shadow-none text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]`}
      style={{
        background: "var(--ct-surface, #161b22)",
        border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 18%, transparent)",
      }}
    >
      <Clock3 size={14} strokeWidth={2.2} />
    </button>
  );
}
