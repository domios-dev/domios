import type { ProviderDescriptor } from "../../lib/types";
import { resolveWorkerSelection } from "./workerSelection";

function provider(id: string, display_name: string, default_model: string, models: string[]): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name,
    transport: "pty",
    system_prompt_delivery: "external",
    available_models: models.map((model) => ({ id: model, display_name: model[0].toUpperCase() + model.slice(1) })),
    default_model,
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
  };
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const providers = [
  provider("provider-a", "Provider A", "model-a", ["model-a", "model-b"]),
  provider("provider-b", "Provider B", "model-c", ["model-c", "model-d"]),
];

const defaultSelection = resolveWorkerSelection(providers, "", "provider-b", "");
assert(defaultSelection.providerLabel === "Provider B", "blank provider resolves to the configured default provider");
assert(defaultSelection.defaultModelLabel === "Model-c", "blank provider label includes its actual default model");

const changedProvider = resolveWorkerSelection(providers, "provider-b", "provider-a", "model-a");
assert(changedProvider.defaultModelLabel === "Model-c", "provider change re-resolves the provider default model");
assert(changedProvider.modelOptions.map((model) => model.id).join(",") === "model-c,model-d", "provider change re-resolves model options");
assert(!changedProvider.modelCompatible, "incompatible selected model is surfaced instead of silently accepted");

const compatibleSelection = resolveWorkerSelection(providers, "provider-a", "provider-b", "model-a");
assert(compatibleSelection.modelCompatible, "provider-compatible selected model remains valid");

const emptyCatalogSelection = resolveWorkerSelection(
  [provider("empty", "Empty", "", [])],
  "empty",
  "empty",
  "removed-model",
);
assert(!emptyCatalogSelection.modelCompatible, "a configured model is unavailable when the catalog is empty");

console.log("✓ Memory worker selection labels and compatibility");
