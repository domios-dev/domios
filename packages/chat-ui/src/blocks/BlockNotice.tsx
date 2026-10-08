import type { LucideIcon } from "lucide-react";
import { SearchX } from "lucide-react";

/** The small note a block shows instead of a card while its state is unknown, unavailable or missing. */
export function BlockNotice({ name, source, label, text, icon: Icon, missing = false }: {
  name: string;
  source: string;
  label: string;
  text: string;
  icon: LucideIcon;
  missing?: boolean;
}) {
  return (
    <div
      data-testid="chat-block"
      data-chat-block={name}
      data-block-missing={missing || undefined}
      role="note"
      aria-label={label}
      className="my-2 flex w-fit max-w-[34rem] items-center gap-2 rounded-lg border px-2.5 py-1.5 text-xs"
      style={{ borderColor: "color-mix(in srgb, var(--ct-muted-fg) 22%, transparent)", color: "var(--ct-muted-fg)" }}
      title={source}
    >
      {missing ? <SearchX size={13} aria-hidden="true" /> : <Icon size={13} aria-hidden="true" />}
      <span>{text}</span>
    </div>
  );
}
