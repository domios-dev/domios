import type { ProviderDescriptor } from "../../lib/types";
import { formatCompact } from "../../lib/formatCompact";

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

type SummaryInput = Pick<ProviderDescriptor, "transport" | "available_models" | "cli_status">;

/** What a provider is and whether it can run, instead of its id. */
export function providerSummary(provider: SummaryInput): string {
  const kind = provider.transport === "chat_backend" ? "Chat backend" : "Terminal agent";
  if (provider.cli_status?.installed === false) return `${kind} · not installed`;
  const models = provider.available_models?.length ?? 0;
  if (models === 0) return kind;
  return `${kind} · ${formatCompact(models)} ${models === 1 ? "model" : "models"}`;
}
