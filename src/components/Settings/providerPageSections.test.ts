import { strict as assert } from "node:assert";
import { test } from "node:test";
import { providerSummary } from "./providerPageSections";

const model = { id: "m", display_name: "M" } as never;

test("a provider card summary counts its models", () => {
  const summary = providerSummary({ transport: "pty", available_models: [model, model], cli_status: undefined });
  assert.match(summary, /2 models/);
});

test("a missing CLI wins over the model count", () => {
  const summary = providerSummary({ transport: "pty", available_models: [model], cli_status: { installed: false } as never });
  assert.match(summary, /not installed/);
  assert.doesNotMatch(summary, /model/);
});

test("chat backends and terminal agents read differently", () => {
  assert.notEqual(
    providerSummary({ transport: "chat_backend", available_models: [], cli_status: undefined }),
    providerSummary({ transport: "pty", available_models: [], cli_status: undefined }),
  );
});

test("model counts are compact", () => {
  const many = Array.from({ length: 1200 }, () => model);
  assert.match(providerSummary({ transport: "chat_backend", available_models: many, cli_status: undefined }), /1\.2k models/);
});
