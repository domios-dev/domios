import assert from "node:assert/strict";
import { test } from "node:test";
import { describeError, type ErrorAction } from "./describeError";

const fixtures: { input: unknown; title: string; actions: ErrorAction[] }[] = [
  { input: { code: "close_failed", raw: "opaque" }, title: "The workspace or Team could not close", actions: ["retry"] },
  { input: { code: "close_inventory_unavailable", raw: "opaque" }, title: "The close status could not be checked", actions: ["retry"] },
  { input: { code: "workspace_cwd_missing", message: "opaque" }, title: "The tab's folder no longer exists", actions: ["choose-folder"] },
  { input: "Failed to spawn powershell: CreateProcessW failed: Неверно задано имя папки. (os error 267)", title: "The tab's folder no longer exists", actions: ["choose-folder"] },
  { input: "spawn shell: ENOTDIR", title: "The tab's folder no longer exists", actions: ["choose-folder"] },
  { input: "spawn shell: cwd /gone does not exist", title: "The tab's folder no longer exists", actions: ["choose-folder"] },
  { input: { code: "attachment_too_large", raw: "opaque" }, title: "The attachment is too large", actions: ["choose-smaller-file"] },
  { input: { status: 413, message: "opaque" }, title: "The attachment is too large", actions: ["choose-smaller-file"] },
  { input: { code: "provider_auth_lost", raw: "opaque" }, title: "Sign in to continue", actions: ["sign-in"] },
  { input: { code: "provider_access_lost", raw: "opaque" }, title: "Access was denied", actions: ["check-access", "retry"] },
  { input: { kind: "runtime_deadline", raw: "opaque" }, title: "The agent runtime did not respond in time", actions: ["retry"] },
  { input: "Deadline: opaque", title: "The agent runtime did not respond in time", actions: ["retry"] },
  { input: "Runtime did not respond in time: opaque", title: "The agent runtime did not respond in time", actions: ["retry"] },
  { input: "spawn shell: os error 5", title: "Access was denied", actions: ["check-access", "retry"] },
  { input: "spawn shell: EACCES", title: "Access was denied", actions: ["check-access", "retry"] },
  { input: "spawn shell: os error 2", title: "The program could not be found", actions: ["check-executable", "retry"] },
  { input: "spawn shell: ENOENT", title: "The program could not be found", actions: ["check-executable", "retry"] },
  { input: { code: "spawn_failed", raw: "opaque" }, title: "The tab could not start", actions: ["retry"] },
  { input: { code: "provider_auth_lost", raw: "spawn shell: os error 267" }, title: "Sign in to continue", actions: ["sign-in"] },
  { input: { body: 'HTTP 422: {"error":{"code":"workspace_cwd_missing","message":"opaque"}}' }, title: "The tab's folder no longer exists", actions: ["choose-folder"] },
  { input: new Error("opaque unknown"), title: "Something went wrong while opening this file", actions: ["retry"] },
  { input: "attachment ENOENT", title: "Something went wrong while opening this file", actions: ["retry"] },
];

for (const [index, fixture] of fixtures.entries()) {
  test(`classifies error fixture ${index + 1}`, () => {
    const view = describeError(fixture.input, { operation: "opening this file" });
    assert.equal(view.title, fixture.title);
    assert.deepEqual(view.actions, fixture.actions);
    assert.ok(view.cause);
  });
}

test("keeps the raw input and folder context available without interpreting them as markup", () => {
  const raw = "CreateProcessW: <opaque> os error 267";
  const cwd = "C:\\deleted\\project";
  const view = describeError(raw, { cwd });
  assert.equal(view.raw, raw);
  assert.ok(view.cause.includes(cwd));
});

test("an explicit typed code takes precedence over JSON-looking raw diagnostics", () => {
  const input = { code: "provider_auth_lost", raw: '{"error":{"code":"workspace_cwd_missing"}}' };
  assert.deepEqual(describeError(input).actions, ["sign-in"]);
  assert.equal(describeError(input).raw, input.raw);
});
