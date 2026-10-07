import { Clock3 } from "lucide-react";

const btnBase =
  "ct-liquid-interactive flex items-center justify-center shrink-0";

export function ComposerScheduleButton({
  onSchedule,
  disabled,
}: {
  onSchedule: () => void;
  disabled: boolean;
}) {
  return (
    <button
      type="button"
      data-testid="composer-schedule-button"
      title="Schedule send"
      aria-label="Schedule send"
      disabled={disabled}
      onMouseDown={(e) => e.preventDefault()}
      onClick={(e) => {
        e.preventDefault();
        onSchedule();
      }}
      className={`${btnBase} ct-composer-round-button h-10 w-10 rounded-full border shadow-none text-[var(--ct-muted-fg)] enabled:hover:text-[var(--ct-fg)] disabled:opacity-40`}
      style={{
        background: "var(--ct-surface, #161b22)",
        border: "1px solid var(--ct-border)",
      }}
    >
      <Clock3 size={18} strokeWidth={2.2} />
    </button>
  );
}
