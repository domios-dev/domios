import { Info } from "lucide-react";
import type { HelpSectionId } from "../../help/types";

export function GuideInfoButton({ label, section, onOpen }: {
  label: string;
  section: HelpSectionId;
  onOpen: (section: HelpSectionId) => void;
}) {
  return <button type="button" aria-label={`About ${label}`} title={`About ${label}`}
    className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]"
    onClick={() => onOpen(section)}><Info size={14} aria-hidden="true" /></button>;
}
