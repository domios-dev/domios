import assert from "node:assert/strict";
import { test } from "node:test";
import { paneStateModel } from "./paneStateModel";

test("missing folders share the same pane model at tab and workspace scope", () => {
  const error = { code: "workspace_cwd_missing", raw: "opaque-refusal" };
  const tab = paneStateModel(error, { scope: "tab", cwd: "/retired/tab" });
  const workspace = paneStateModel(error, { scope: "workspace", cwd: "/retired/workspace" });
  assert.equal(tab.icon, "folder-off");
  assert.equal(workspace.icon, "folder-off");
  assert.equal(tab.title, workspace.title);
  assert.notEqual(tab.sentence, workspace.sentence);
  assert.equal(tab.path, "/retired/tab");
  assert.equal(workspace.path, "/retired/workspace");
  assert.deepEqual(tab.actions, ["choose-folder"]);
  assert.deepEqual(workspace.actions, ["choose-folder"]);
  assert.equal(tab.details, error.raw);
});

test("generic launch failure offers retry without folder recovery", () => {
  const view = paneStateModel({ code: "spawn_failed", raw: "opaque-launch" }, { scope: "tab", cwd: "/project" });
  assert.equal(view.icon, "alert");
  assert.equal(view.path, "/project");
  assert.deepEqual(view.actions, ["retry"]);
  assert.equal(view.details, "opaque-launch");
});

test("authentication failure offers sign-in rather than launch or folder recovery", () => {
  const view = paneStateModel({ code: "auth_required", raw: "opaque-auth" });
  assert.equal(view.icon, "sign-in");
  assert.equal(view.path, null);
  assert.deepEqual(view.actions, ["sign-in"]);
  assert.equal(view.details, "opaque-auth");
});

test("a runtime response deadline is pending without failure actions", () => {
  const pending = paneStateModel({ code: "runtime_deadline", raw: "opaque" });
  assert.equal(pending.pending, true);
  assert.deepEqual(pending.actions, []);
  const terminal = paneStateModel("ProcessExited: runtime exited");
  assert.equal(terminal.pending, false);
  assert.deepEqual(terminal.actions, ["retry"]);
});
