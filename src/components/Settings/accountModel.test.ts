import assert from "node:assert/strict";
import { accountModel, accountFailure, accountLoginPath } from "./accountModel";
assert.deepEqual(accountModel({ identity: null, pending: false }), { kind: "signed_out" });
assert.deepEqual(accountModel({ identity: null, pending: true }), { kind: "pending" });
const identity = { uid: "fixture", email: "fixture@example.test", display_name: "Fixture", provider: "fake" };
assert.deepEqual(accountModel({ identity, pending: false }), { kind: "signed_in", identity });
assert.deepEqual(accountModel({ identity, pending: true }), { kind: "pending" });
const safeError = accountFailure(new Error("opaque-sensitive-response"));
assert.equal(safeError.kind, "error");
assert.ok(safeError.kind === "error" && !safeError.message.includes("opaque-sensitive-response"));
assert.equal(accountFailure(undefined).kind, "error");
console.log("7 account model assertions passed");

assert.equal(accountLoginPath(), "/api/account/login");
assert.equal(accountLoginPath({provider:"google",mode:"signup"}), "/api/account/login?provider=google&mode=signup");
assert.equal(accountLoginPath({provider:"email",mode:"signin"}), "/api/account/login?provider=email&mode=signin");
console.log("3 login hint URL assertions passed");

assert.deepEqual(accountModel({ identity, pending: false, session_only: true }), { kind: "signed_in", identity, sessionOnly: true });
assert.deepEqual(accountModel({ identity: null, pending: false, session_only: true }), { kind: "signed_out" });
console.log("2 session-only account assertions passed");

for (const stage of ["preparation", "browser", "callback", "redemption", "credential_save", "publication"] as const) {
  const status = { identity: null, pending: false, last_sign_in_failure: { stage, error: "provider" as const } };
  const failure = accountModel(status);
  assert.equal(failure.kind, "error");
  assert.deepEqual(accountModel(structuredClone(status)), failure);
  assert.deepEqual(accountModel({ ...status, pending: true }), { kind: "pending" });
  assert.deepEqual(accountModel({ ...status, last_sign_in_failure: null }), { kind: "signed_out" });
}
console.log("24 retained sign-in failure assertions passed");

assert.deepEqual(accountModel({ identity: null, pending: false, session_revoked: true }), { kind: "signed_out", sessionRevoked: true });
