import type { ProviderDescriptor } from "../../lib/types";
import { AgentIcon } from "../AgentIcon";
import { kindDisplayName } from "../../lib/roleKind";
import { effortLabel, managerDecision, roleSameAs, splitModel } from "../../lib/teamTemplates/startModel";
import type { TemplatePreview } from "../../lib/transport/teamTemplates";
type TemplateRoleSummary = import("../../../gen/TemplateRoleSummary").TemplateRoleSummary;

const CAPSULE = "ml-auto inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-full px-2 py-0.5 text-[10.5px]";
const UNSET = `${CAPSULE} border border-dashed border-[color-mix(in_srgb,var(--ct-fg)_18%,transparent)] text-[var(--ct-muted-fg)]`;

export function ModelCapsule({ preview, role, providers }: { preview: TemplatePreview; role: TemplateRoleSummary; providers: readonly ProviderDescriptor[] }) {
  const decision = role.kind === "manager" ? managerDecision(preview.form.pickers, role) : role.model;
  const target = roleSameAs(role);
  if (target) return <span className={UNSET}>Same as {kindDisplayName(target)}</span>;
  if (!decision?.choice) return <span className={UNSET}>You pick</span>;
  const { providerId, modelId } = splitModel(decision.choice.model);
  const model = providers.find((provider) => provider.id === providerId)?.available_models.find((entry) => entry.id === modelId)?.display_name ?? modelId;
  const effort = decision.effort?.applied;
  return (
    <span className={`${CAPSULE} bg-[color-mix(in_srgb,var(--ct-fg)_6%,transparent)] font-mono`}>
      <AgentIcon providerId={providerId} className="text-[11px]" />{effort ? `${model} · ${effortLabel(effort)}` : model}
    </span>
  );
}

