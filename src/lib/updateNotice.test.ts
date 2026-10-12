import assert from "node:assert/strict";
import { test } from "node:test";
import { updateNotice } from "./updateNotice";

const input = { title: "Update failed", body: "fallback", failed: true, error: "Tab Euler (tab-1): process agent.exe PID 42 could not terminate", latestVersion: "1.14.4" };
test("failure preserves the exact blocker and offers retry", () => {
  assert.deepEqual(updateNotice(input), { title: "Update failed v1.14.4", body: input.error, severity: "Error", level: "failure", retry: true });
});
test("foreign process identity reaches the notice unchanged", () => {
  const error = "Foreign install-file holder: editor.exe (PID 83)";
  assert.equal(updateNotice({ ...input, error }).body, error);
});
test("progress does not offer retry", () => {
  assert.deepEqual(updateNotice({ ...input, title: "Downloading", failed: false, error: null, latestVersion: null }), { title: "Downloading", body: "fallback", severity: "Info", level: "verbose", retry: false });
});
