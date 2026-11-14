import { useState } from "react";
import type { TeamDocument } from "../../../gen/TeamDocument";
import type { TemplatePreview } from "../../lib/transport/teamTemplates";
import type { ProviderDescriptor } from "../../lib/types";
import { DropdownSelect } from "../shared/DropdownMenu";
import { ModelCapsule } from "./TemplateRoleModel";
import { reviewRoles, roleHeading } from "./templateRoles";
import { TemplatePackageText } from "./TemplatePackageText";

export function TemplateInstructions({ preview, document, providers }: { preview: TemplatePreview; document?: TeamDocument; providers: readonly ProviderDescriptor[] }) {
  const roles = reviewRoles(preview.form.roles);
  const [selected, setSelected] = useState(roles[0]?.role ?? "");
  const role = roles.find((candidate) => candidate.role === selected) ?? roles[0];
  if (!role) return <p>No roles.</p>;
  const heading = roleHeading(role);
  const declared = document?.roles[role.role];
  return <div className="flex flex-col gap-4" data-testid="template-instructions">
    <DropdownSelect aria-label="Team role" value={role.role} onChange={setSelected} focusOnPointerOpen options={roles.map((candidate) => ({
      value: candidate.role, label: `${roleHeading(candidate).name} · ${candidate.kind === "manager" ? "Manager" : "Worker"}`,
    }))} />
    <div className="flex flex-wrap items-center gap-3"><h3 className="text-base font-medium">{heading.name}{heading.slot && ` · ${heading.slot}`}</h3><ModelCapsule preview={preview} role={role} providers={providers} /></div>
    {declared && <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-[12px]">
      <dt className="text-[var(--ct-muted-fg)]">Files</dt><dd className="break-words">{declared.files?.join(", ") || "None"}</dd>
      <dt className="text-[var(--ct-muted-fg)]">Skills</dt><dd className="break-words">{declared.skills?.join(", ") || "None"}</dd>
    </dl>}
    <div aria-label={`${heading.name} instructions`}><TemplatePackageText key={role.role} template={preview.form.template} target={{ role: role.role }} /></div>
  </div>;
}
