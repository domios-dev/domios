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
  provider("claude", "Claude", "sonnet", ["sonnet", "haiku"]),
  provider("cursor", "Cursor", "auto", ["auto", "composer"]),
];

const defaultSelection = resolveWorkerSelection(providers, "", "cursor", "");
assert(defaultSelection.providerLabel === "Cursor", "blank provider resolves to the configured default provider");
assert(defaultSelection.defaultModelLabel === "Auto", "blank provider label includes its actual default model");

const changedProvider = resolveWorkerSelection(providers, "cursor", "claude", "sonnet");
assert(changedProvider.defaultModelLabel === "Auto", "provider change re-resolves the provider default model");
assert(changedProvider.modelOptions.map((model) => model.id).join(",") === "auto,composer", "provider change re-resolves model options");
assert(!changedProvider.modelCompatible, "incompatible selected model is surfaced instead of silently accepted");

const compatibleSelection = resolveWorkerSelection(providers, "claude", "cursor", "sonnet");
assert(compatibleSelection.modelCompatible, "provider-compatible selected model remains valid");

console.log("✓ Memory worker selection labels and compatibility");
