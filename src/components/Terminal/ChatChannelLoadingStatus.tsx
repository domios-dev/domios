import { Clock3 } from "lucide-react";

export function ChatChannelLoadingStatus({ label }: { label: string }) {
  return (
    <span
      role="img"
      aria-label={label}
      title={label}
      className="inline-flex shrink-0 items-center text-[var(--ct-muted-fg)]"
    >
      <Clock3 size={12} aria-hidden="true" />
    </span>
  );
}
