import assert from "node:assert/strict";
import { test } from "node:test";
import { providerPageSections } from "../providerPageSections";
import {
  addRequest,
  applyDiscovered,
  discoverPlan,
  draftFromEndpoint,
  draftProblem,
  emptyDraft,
  errorKind,
  errorLine,
  selectableSecrets,
  toggleModel,
  updateRequest,
  type ModelEndpoint,
} from "./modelEndpointsModel";

const endpoint: ModelEndpoint = {
  id: "fixture",
  kind: "openai-compatible",
  baseUrl: "http://localhost:1234/v1",
  models: ["alpha", "beta"],
  secretName: "domios/model-provider/fixture",
};

test("placement follows declared capabilities, never provider identity", () => {
  assert.deepEqual(providerPageSections({ has_hooks: false, supports_model_endpoints: true }), { integration: false, modelEndpoints: true });
  assert.deepEqual(providerPageSections({ has_hooks: true }), { integration: true, modelEndpoints: false });
});

test("a typed key travels as the request value; a saved secret travels by name", () => {
  const draft = { ...emptyDraft(), id: " fixture ", baseUrl: " http://localhost:1234/v1 ", keyValue: "opaque-input" };
  assert.deepEqual(addRequest(draft), {
    action: "add",
    provider: { id: "fixture", kind: "openai-compatible", baseUrl: "http://localhost:1234/v1", models: [] },
    value: "opaque-input",
  });
  const saved = addRequest({ ...draft, keySource: "saved", savedSecret: "stored-name" });
  assert.equal(saved.action, "add_from_secret");
  assert.equal("value" in saved, false);
});

test("discovered models are chosen by checkbox and nothing checked means all", () => {
  let draft = applyDiscovered({ ...emptyDraft(), id: "fixture", baseUrl: endpoint.baseUrl, keyValue: "k" }, ["alpha", "beta", "alpha"]);
  assert.deepEqual(draft.discovered, ["alpha", "beta"]);
  assert.deepEqual(addRequest(draft).provider.models, []);
  draft = toggleModel(draft, "beta");
  assert.deepEqual(addRequest(draft).provider.models, ["beta"]);
  draft = applyDiscovered(draft, ["alpha"]);
  assert.deepEqual(draft.selected, []);
});

test("editing keeps the saved key unless a replacement is typed", () => {
  const draft = draftFromEndpoint(endpoint);
  assert.equal("value" in updateRequest(draft), false);
  assert.deepEqual(updateRequest(draft).provider.models, ["alpha", "beta"]);
  assert.equal(updateRequest({ ...draft, keySource: "new", keyValue: "rotated-input" }).value, "rotated-input");
});

test("discovery uses the typed key, a saved secret, or the stored key at an unchanged address", () => {
  const base = { ...emptyDraft(), baseUrl: endpoint.baseUrl };
  assert.equal(discoverPlan(base).kind, "blocked");
  assert.equal(discoverPlan({ ...base, baseUrl: "not a url", keyValue: "k" }).kind, "blocked");
  const typed = discoverPlan({ ...base, keyValue: "k" });
  assert.ok(typed.kind === "request" && "value" in typed.request);
  const saved = discoverPlan({ ...base, keySource: "saved", savedSecret: "stored-name" });
  assert.ok(saved.kind === "request" && "source_secret" in saved.request);
  const edit = draftFromEndpoint(endpoint);
  assert.deepEqual(discoverPlan(edit, endpoint), { kind: "stored", id: "fixture" });
  assert.equal(discoverPlan({ ...edit, baseUrl: "http://elsewhere:9/v1" }, endpoint).kind, "blocked");
});

test("validation refuses an unusable draft and accepts a complete one", () => {
  assert.ok(draftProblem({ ...emptyDraft(), id: "Bad Name" }, "add"));
  assert.ok(draftProblem({ ...emptyDraft(), id: "ok", baseUrl: "ftp://x" }, "add"));
  assert.ok(draftProblem({ ...emptyDraft(), id: "ok", baseUrl: endpoint.baseUrl }, "add"));
  assert.equal(draftProblem({ ...emptyDraft(), id: "ok", baseUrl: endpoint.baseUrl, keyValue: "k" }, "add"), null);
  assert.equal(draftProblem(draftFromEndpoint(endpoint), "edit"), null);
});

test("managed copies are not offered as saved secrets", () => {
  assert.deepEqual(selectableSecrets(["zeta", "domios/model-provider/x", "alpha", "zeta"]), ["alpha", "zeta"]);
});

test("backend error kinds map to distinct guidance from Tauri and web errors alike", () => {
  assert.equal(errorKind("unauthorized"), "unauthorized");
  assert.equal(errorKind(new Error('/api/model-providers → HTTP 400: {"kind":"connection"}')), "connection");
  assert.notEqual(errorLine("unauthorized"), errorLine("connection"));
  assert.equal(errorLine(new Error("opaque-raw-detail")).includes("opaque-raw-detail"), false);
});
