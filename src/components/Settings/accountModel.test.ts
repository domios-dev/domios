import assert from "node:assert/strict";
import { accountModel, accountFailure, accountLoginPath } from "./accountModel";
assert.deepEqual(accountModel({ identity: null, pending: false }), { kind: "signed_out" });
assert.deepEqual(accountModel({ identity: null, pending: true }), { kind: "pending" });
const identity = { uid: "fixture", email: "fixture@example.test", display_name: "Fixture", provider: "fake" };
assert.deepEqual(accountModel({ identity, pending: false }), { kind: "signed_in", identity });
assert.deepEqual(accountModel({ identity, pending: true }), { kind: "pending" });
assert.deepEqual(accountFailure(new Error("fixture failure")), { kind: "error", message: "fixture failure" });
assert.equal(accountFailure(undefined).kind, "error");
console.log("6 account model assertions passed");

assert.equal(accountLoginPath(), "/api/account/login");
assert.equal(accountLoginPath({provider:"google",mode:"signup"}), "/api/account/login?provider=google&mode=signup");
assert.equal(accountLoginPath({provider:"email",mode:"signin"}), "/api/account/login?provider=email&mode=signin");
console.log("3 login hint URL assertions passed");
