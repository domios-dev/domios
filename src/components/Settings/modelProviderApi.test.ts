import assert from "node:assert/strict";
import { test } from "node:test";
import { modelProviderAddRequest } from "./modelProviderApi";

test("Settings sends a saved secret reference and normalized model IDs", () => {
  const request = modelProviderAddRequest({ name: " endpoint ", kind: "anthropic", baseUrl: " https://example.test/v1 ", sourceSecret: " source ", models: "first, second first\nthird" });
  assert.deepEqual(request, {
    action: "add_from_secret", source_secret: "source",
    provider: { id: "endpoint", kind: "anthropic", baseUrl: "https://example.test/v1", models: ["first", "second", "third"] },
  });
  assert.equal("value" in request, false);
});

test("omitted models request discovery", () => {
  assert.deepEqual(modelProviderAddRequest({ name: "endpoint", kind: "openai-compatible", baseUrl: "https://example.test", sourceSecret: "source", models: " " }).provider.models, []);
});
