import { expect, it } from "vitest";
import {
  choiceForSessionSection,
  defaultNewSessionChoice,
  readPersistedNewSessionChoice,
  resolvePersistedChoice,
  resolveProviderModel,
} from "./newSessionChoice";
import type { ProviderDescriptor } from "./types";

function makeProvider(id: string, availability: ProviderDescriptor["availability"] = "available", model = `${id}-model`): ProviderDescriptor {
  return {
    id,
    display_name: id,
    availability,
    default_model: model,
    available_models: [{ id: model, display_name: model }],
    reasoning_efforts: [],
  } as unknown as ProviderDescriptor;
}

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

it("never defaults to or retains an unavailable provider", () => {
  const cursor = makeProvider("cursor", "unavailable");
  const claude = makeProvider("claude", "available");
  const disabled = new Set(["cursor"]);

  expect(defaultNewSessionChoice([cursor, claude])).toMatchObject({
    mode: "pair",
    orchestratorProviderId: "claude",
    orchestratorModelId: "claude-model",
    plannerProviderId: "claude",
    plannerModelId: "claude-model",
  });

  // Also skips providers flagged unavailable via install probe even if descriptor availability is unknown.
  const unknownCursor = makeProvider("cursor", "unknown");
  expect(defaultNewSessionChoice([unknownCursor, claude], disabled)).toMatchObject({
    mode: "pair",
    orchestratorProviderId: "claude",
    plannerProviderId: "claude",
  });

  // Never retains a persisted unavailable provider for Single or Team.
  expect(resolvePersistedChoice([unknownCursor, claude], { mode: "single", providerId: "cursor", modelId: "cursor-model", preset: "saved" }, disabled)).toEqual({
    mode: "single",
    providerId: "claude",
    modelId: "claude-model",
    effortId: undefined,
  });
  expect(resolvePersistedChoice([unknownCursor, claude], {
    mode: "pair",
    orchestratorProviderId: "cursor",
    orchestratorModelId: "cursor-model",
    plannerProviderId: "cursor",
    plannerModelId: "cursor-model",
  }, disabled)).toMatchObject({
    mode: "pair",
    orchestratorProviderId: "claude",
    plannerProviderId: "claude",
  });

  // Section conversion skips unavailable providers too.
  expect(choiceForSessionSection({ mode: "terminal" }, "general", [unknownCursor, claude], disabled)).toEqual({
    mode: "single",
    providerId: "claude",
    modelId: "claude-model",
    effortId: undefined,
  });

  // When no providers are installed, nothing unavailable is preselected.
  expect(defaultNewSessionChoice([cursor])).toEqual({
    mode: "pair",
    orchestratorProviderId: "",
    orchestratorModelId: undefined,
    orchestratorEffortId: undefined,
    plannerProviderId: "",
    plannerModelId: undefined,
    plannerEffortId: undefined,
  });

  window.localStorage.clear();
  expect(readPersistedNewSessionChoice([unknownCursor, claude], disabled)).toMatchObject({
    mode: "pair",
    orchestratorProviderId: "claude",
    plannerProviderId: "claude",
  });
});
