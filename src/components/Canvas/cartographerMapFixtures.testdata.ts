import type { ProviderDescriptor } from "../../lib/types";

export const CARTOGRAPHER_TEST_PROVIDER: ProviderDescriptor = {
  schema_version: 1,
  id: "fixture-provider",
  display_name: "Fixture provider",
  transport: "pty",
  system_prompt_delivery: "external",
  availability: "available",
  available_models: [{ id: "fixture-model", display_name: "Fixture model" }],
  default_model: "fixture-model",
  settings_schema: [],
  notifications_schema: [],
  supports_lazy_skills: false,
  has_hooks: false,
};
