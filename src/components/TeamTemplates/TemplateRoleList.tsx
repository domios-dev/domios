import { Users } from "lucide-react";
import type { ProviderDescriptor } from "../../lib/types";
import type { TemplatePreview } from "../../lib/transport/teamTemplates";
import { lacksAuthority, roleTiming } from "../../lib/teamTemplates/startModel";
import { managerIcon } from "./TemplateManagers";
import { ModelCapsule } from "./TemplateRoleModel";
import { roleHeading } from "./templateRoles";

type TemplateRoleSummary = import("../../../gen/TemplateRoleSummary").TemplateRoleSummary;

export function TemplateRoleList({ title, roles, preview, providers }: { title: string; roles: TemplateRoleSummary[]; preview: TemplatePreview; providers: readonly ProviderDescriptor[] }) {
  if (roles.length === 0) return null;
  return (
    <section aria-label={title} className="flex flex-col gap-0.5">
      <div className="text-[11px] font-medium text-[var(--ct-muted-fg)]">{title}</div>
      {roles.map((role) => {
        const heading = roleHeading(role);
        const start = [role.description?.trim(), roleTiming(role.start) ?? "Starts with the team"].filter(Boolean).join(" · ");
        return (
          <div key={`${role.kind}:${role.role}`} className="flex flex-wrap items-center gap-2.5 py-1 text-[12px]" data-role-kind={role.kind}>
            <span aria-hidden="true" className="grid h-[22px] w-[22px] shrink-0 place-items-center rounded-[7px] bg-[color-mix(in_srgb,var(--ct-accent)_12%,transparent)] text-[var(--ct-accent)]">
              {role.kind === "manager" ? managerIcon(role.role) : <Users size={13} />}
            </span>
            <div className="min-w-0 flex-1">
              <div>{heading.name}{heading.slot && <span className="text-[var(--ct-muted-fg)]"> · {heading.slot}</span>}</div>
              <div className="text-[11.5px] text-[var(--ct-muted-fg)]">{start}</div>
              {lacksAuthority(role) && <div className="text-[11px] text-[var(--ct-muted-fg)]" data-authority="none">No Domios authority</div>}
            </div>
            <ModelCapsule preview={preview} role={role} providers={providers} />
          </div>
        );
      })}
    </section>
  );
}

