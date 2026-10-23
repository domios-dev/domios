import assert from "node:assert/strict";
import { test } from "node:test";
import { describeError, isPendingRuntimeDeadline, type ErrorAction } from "./describeError";
import { reorderFailureToast } from "../store/multiplexer/reorderFailure";

test("reorder failures retain their reason through transport and notification serialization", () => {
  const reason = "opaque-reorder-reason-27";
  const body = JSON.stringify({ ok: false, error: { code: "reorder_failed", message: reason } });
  for (const error of [{ body, status: 400 }, new Error(`HTTP 400: ${body}`), new Error(reason), reason]) {
    const notice = reorderFailureToast("workspace", error);
    assert.equal(notice.workspaceId, "workspace");
    const description = describeError(notice.detail);
    assert.equal(description.cause, reason);
    assert.deepEqual(description.actions, ["retry"]);
    assert.equal(description.pending, undefined);
  }
});

const fixtures: { input: unknown; title: string; actions: ErrorAction[] }[] = [
  { input: { code: "constructor", raw: "opaque" }, title: "Something went wrong while opening this file", actions: ["retry"] },
  { input: { code: "memory_run_incomplete", raw: "opaque" }, title: "The memory run needs attention", actions: ["retry"] },
  { input: { code: "frontend_version_mismatch", raw: "opaque" }, title: "The interface needs an update", actions: ["retry"] },
  { input: { code: "frontend_crash", raw: "opaque" }, title: "The interface could not render", actions: [] },
  { input: { code: "plugin_load_failed", raw: "opaque" }, title: "The plugin could not load", actions: ["retry"] },
  { input: { code: "plugin_reload_failed", raw: "opaque" }, title: "The plugin could not reload", actions: ["retry"] },
  { input: { code: "plugin_removed", raw: "opaque" }, title: "This chat's plugin was removed", actions: [] },
  { input: { code: "mesh_partial_failure", raw: "opaque" }, title: "Some mesh resources could not be refreshed", actions: ["retry"] },
  { input: { code: "terminal_input_failed", raw: "" }, title: "Terminal input failed", actions: ["retry"] },
  { input: { code: "automation_unavailable", raw: "" }, title: "Automations could not be loaded", actions: ["retry"] },
  { input: { code: "blueprint_unavailable", raw: "" }, title: "The Blueprint is unavailable", actions: ["retry"] },

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
  { input: { kind: "runtime_deadline", raw: "opaque" }, title: "The agent runtime is taking longer than usual", actions: [] },
  { input: "Deadline: opaque", title: "The agent runtime is taking longer than usual", actions: [] },
  { input: "Runtime did not respond in time: opaque", title: "The agent runtime is taking longer than usual", actions: [] },
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

test("model switch incidents remain failures when their details mention a deadline", () => {
  for (const input of ["mid-turn model switch incident at effort:UnknownPrompt: an unrecognized confirmation is waiting; deadline expired", {kind:"during_turn_switch_incident",raw:"an unrecognized confirmation is waiting; deadline expired"}]) {
    const view = describeError(input);
    assert.equal(view.title, "An unrecognized confirmation is waiting");
    assert.equal(view.pending, undefined);
    assert.equal(isPendingRuntimeDeadline(input), false);
  }
});


test("provider switch failures name auth deadlines and bound transport details", () => {
  for (const input of ["provider_switch_auth_timeout: opaque", { code: "provider_switch_auth_timeout", message: "opaque" }, new Error("provider_switch_auth_timeout: opaque")]) {
    const view = describeError(input, { operation: "switching the provider" });
    assert.match(view.title, /authentication check timed out/);
    assert.deepEqual(view.actions, ["retry"]);
    assert.equal(view.pending, undefined);
  }
  const unavailable = describeError("provider_switch_unavailable: Provider not installed: opaque shell", { operation: "switching the provider" });
  assert.equal(unavailable.title, "The target provider is unavailable");
  const failed = describeError("shell spawn failure " + "opaque".repeat(1000), { operation: "switching the provider" });
  assert.equal(failed.title, "The provider could not be switched");
  assert.ok(failed.raw.length <= 2048);
});
