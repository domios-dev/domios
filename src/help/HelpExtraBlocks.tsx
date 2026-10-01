import type { ReactNode } from "react";
import { ChevronRight, SlidersHorizontal } from "lucide-react";
import type { HelpSettingsTarget } from "./types";

export function SettingsLinkButton({
  label,
  target,
  onOpen,
}: {
  label: string;
  target: HelpSettingsTarget;
  onOpen: (target: HelpSettingsTarget) => void;
}) {
  return (
    <button
      type="button"
      data-testid="help-settings-link"
      data-help-settings-target={target}
      onClick={() => onOpen(target)}
      className="flex w-full items-center gap-2.5 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/40 px-3 py-2.5 text-left text-sm transition-colors hover:border-[var(--ct-accent)]/50 hover:bg-[var(--ct-hover)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]"
    >
      <span
        aria-hidden="true"
        className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-[color-mix(in_srgb,var(--ct-accent)_14%,transparent)] text-[var(--ct-accent)]"
      >
        <SlidersHorizontal size={14} />
      </span>
      <span className="min-w-0 flex-1 font-medium text-[var(--ct-fg)]">{label}</span>
    </button>
  );
}

export function HelpDetails({ summary, children }: { summary: string; children: ReactNode }) {
  return (
    <details
      className="group rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/30"
      data-testid="help-details"
    >
      <summary className="flex cursor-pointer list-none items-center gap-2 rounded-xl px-3 py-2.5 text-sm font-medium text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)] [&::-webkit-details-marker]:hidden">
        <ChevronRight
          size={14}
          aria-hidden="true"
          className="shrink-0 text-[var(--ct-muted-fg)] transition-transform group-open:rotate-90 motion-reduce:transition-none"
        />
        {summary}
      </summary>
      <div className="space-y-3 px-3 pb-3 pt-1">{children}</div>
    </details>
  );
}

export function HelpMeasurement({ label, text }: { label: string; text: string }) {
  return (
    <div
      className="rounded-lg border border-dashed border-[var(--ct-border)] px-3 py-2.5 text-sm"
      data-testid="help-measurement"
    >
      <span className="block font-medium text-[var(--ct-fg)]">{label}</span>
      <span className="mt-0.5 block text-[var(--ct-muted-fg)] leading-relaxed">{text}</span>
    </div>
  );
}
