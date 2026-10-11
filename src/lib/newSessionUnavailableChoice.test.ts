import { expect, it } from "vitest";
import { resolveProviderModel, resolvePersistedChoice } from "./newSessionChoice";
import type { ProviderDescriptor } from "./types";

it("clears a remembered unavailable model and preset", () => {
  const provider = {
    id: "fixture", default_model: "dead",
    available_models: [{ id: "dead", display_name: "Dead", available: false }, { id: "working", display_name: "Working" }],
    reasoning_efforts: [],
  } as unknown as ProviderDescriptor;
  expect(resolvePersistedChoice([provider], { mode: "single", providerId: "fixture", modelId: "dead", preset: "old" })).toEqual({
    mode: "single", providerId: "fixture", modelId: "working", effortId: undefined,
  });
  expect(resolveProviderModel({ ...provider, available_models: [] }, "dead")).toBeUndefined();
});
