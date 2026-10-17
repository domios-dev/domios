import type { RuntimeServiceTier, RuntimeSettings } from "@codeterm/plugin-sdk";
import { list, object, text } from "./runtimeProtocol";

export const codexServiceTiers: readonly RuntimeServiceTier[] = [
  {id:"priority",name:"Fast",description:"",unavailableReason:"Availability not confirmed"},
  {id:"ultrafast",name:"Ultrafast",description:"",unavailableReason:"Not available on your plan"},
];

export function serviceTierSelection(value: unknown): value is NonNullable<RuntimeSettings["serviceTier"]> {
  const selection = object(value);
  return value !== null && (selection.id === null || typeof selection.id === "string" && selection.id.length > 0 && selection.id.length <= 128);
}

export function modelServiceTiers(models: unknown, requirements: unknown): Record<string, RuntimeServiceTier[]> {
  const response = object(requirements);
  const features = object(object(response.requirements).featureRequirements);
  const fast = features.fast_mode !== false;
  const ultra = features.ultrafast_mode !== false && (response.supportsIndependentSpeedModes === true || fast);
  const knownRequirements = requirements !== undefined;
  const result: Record<string, RuntimeServiceTier[]> = Object.create(null);
  for (const raw of list(models)) {
    const model = object(raw);
    const id = text(model.model) ?? text(model.id);
    if (!id) continue;
    const advertised = list(model.serviceTiers).map(object).flatMap(tier => {
      const tierId = text(tier.id), name = text(tier.name), description = text(tier.description);
      if (!tierId || !name || typeof tier.description !== "string") return [];
      const allowed = knownRequirements && (tierId === "flex" || (tierId === "ultrafast" ? ultra : fast));
      return [{id:tierId,name,description:description ?? "",...(!allowed ? {unavailableReason:knownRequirements ? "Disabled by your administrator" : "Availability not confirmed"} : {})}];
    });
    result[id] = [...advertised,...codexServiceTiers.filter(tier => !advertised.some(item => item.id === tier.id))];
  }
  return result;
}

export function tierAvailable(settings: RuntimeSettings, tier: NonNullable<RuntimeSettings["serviceTier"]>): boolean {
  return tier.id === null || !!settings.serviceTierAvailability?.[settings.model ?? ""]?.some(option => option.id === tier.id && !option.unavailableReason);
}
