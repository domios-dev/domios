import type { ProviderDescriptor } from "../../lib/types";

export interface ProviderPageSections {
  integration: boolean;
  modelEndpoints: boolean;
}

export function providerPageSections(
  provider: Pick<ProviderDescriptor, "has_hooks" | "supports_model_endpoints">,
): ProviderPageSections {
  return {
    integration: provider.has_hooks === true,
    modelEndpoints: provider.supports_model_endpoints === true,
  };
}
