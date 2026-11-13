import { kindDisplayName, roleDisplayLabel } from "../../lib/roleKind";
import { managerRoles } from "../../lib/teamTemplates/startModel";

type TemplateRoleSummary = import("../../../gen/TemplateRoleSummary").TemplateRoleSummary;

/** Managers in slot order, then workers, as the review lists them. */
export function reviewRoles(roles: readonly TemplateRoleSummary[]): TemplateRoleSummary[] {
  return [...managerRoles(roles).filter((role) => roles.includes(role)), ...roles.filter((role) => role.kind === "worker")];
}

/** "Lead" with the muted slot "Orchestrator"; an undeclared manager is just its slot, a worker its humanized name. */
export function roleHeading(role: TemplateRoleSummary): { name: string; slot: string | null } {
  if (role.kind !== "manager") return { name: roleDisplayLabel(role.role, role.label), slot: null };
  const slot = kindDisplayName(role.role);
  const name = roleDisplayLabel(role.role, role.label);
  return name === slot ? { name: slot, slot: null } : { name, slot };
}
