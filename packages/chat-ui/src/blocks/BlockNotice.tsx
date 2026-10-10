import type { LucideIcon } from "lucide-react";
import { SearchX } from "lucide-react";

/** The small note a block shows instead of a card while its state is unknown, unavailable or missing. */
export function BlockNotice({ name, source, label, text, icon: Icon, missing = false, missingIcon: MissingIcon = SearchX }: {
  name: string;
  source: string;
  label: string;
  text: string;
  icon: LucideIcon;
  missing?: boolean;
  /** What stands in when the object is not offered, e.g. a lock for a refused secret. */
  missingIcon?: LucideIcon;
}) {
  return (
    <div
      data-testid="chat-block"
      data-chat-block={name}
      data-block-missing={missing || undefined}
      role="note"
      aria-label={label}
      className="ct-card ct-card--quiet my-2 flex w-fit max-w-[34rem] items-center gap-2 px-3 py-1.5 text-xs"
      style={{ color: "var(--ct-muted-fg)" }}
      title={source}
    >
      {missing ? <MissingIcon size={13} aria-hidden="true" /> : <Icon size={13} aria-hidden="true" />}
      <span>{text}</span>
    </div>
  );
}
