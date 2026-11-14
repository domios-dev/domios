import { Eye } from "lucide-react";
import type { TemplatePreview } from "../../lib/transport/teamTemplates";
import { reviewRoles } from "./templateRoles";

export function ReadInstructionsButton({ preview, onClick }: { preview: TemplatePreview; onClick: () => void }) {
  const count = reviewRoles(preview.form.roles).length;
  if (!count) return null;
  return <button type="button" className="inline-flex self-start items-center gap-1 text-[12px] text-[var(--ct-accent)] hover:underline" onClick={onClick}>
    <Eye size={12} aria-hidden="true" />Read the instructions for {count === 1 ? "1 role" : `${count} roles`}
  </button>;
}
