import assert from "node:assert/strict";
import { test } from "node:test";
import { resolveModelDisplayName } from "./agentModelName";

const catalogue = [
  {
    id: "first",
    available_models: [
      { id: "shared-id", display_name: "First Display Name", badge: "Featured", available: false },
      { id: "upstream/other-model", display_name: "Other Model" },
    ],
  },
  {
    id: "second",
    available_models: [{ id: "shared-id", display_name: "Second Display Name" }],
  },
];

test("model display names come from the owning provider's catalogue", () => {
  assert.equal(resolveModelDisplayName(catalogue, "first", "shared-id"), "First Display Name");
  assert.equal(resolveModelDisplayName(catalogue, "second", "shared-id"), "Second Display Name");
  assert.equal(resolveModelDisplayName(catalogue, "first", "upstream/other-model"), "Other Model");
});

test("a missing catalogue entry preserves the reported model ID", () => {
  assert.equal(resolveModelDisplayName(catalogue, "second", "upstream/other-model"), "upstream/other-model");
  assert.equal(resolveModelDisplayName(catalogue, "first", "gpt-7.2-sol"), "gpt-7.2-sol");
  assert.equal(resolveModelDisplayName(catalogue, null, "shared-id"), "shared-id");
  assert.equal(resolveModelDisplayName(catalogue, "missing", "shared-id"), "shared-id");
  assert.equal(resolveModelDisplayName([], "first", "shared-id"), "shared-id");
});
