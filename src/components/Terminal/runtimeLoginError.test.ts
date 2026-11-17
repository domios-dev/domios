// Run: npx tsx src/components/Terminal/runtimeLoginError.test.ts
import assert from "node:assert/strict";
import { TransportError } from "../../lib/transportClient";
import { runtimeLoginErrorMessage } from "./runtimeLoginError";

const generic = "Sign-in failed. Try again.";
const pending = "A sign-in for this provider is already pending in tab opaque-tab. Finish or cancel it there before trying again.";
assert.equal(runtimeLoginErrorMessage(new TransportError(409, "/api/tabs/opaque-tab/agent/runtime/login", pending)), pending);
assert.equal(runtimeLoginErrorMessage("Sign-in failed: opaque provider reason"), "Sign-in failed: opaque provider reason");

const envelope = JSON.stringify({ error: { code: "internal", message: "Sign-in failed: opaque reason" } });
assert.equal(runtimeLoginErrorMessage(new TransportError(409, "/login", envelope)), "Sign-in failed: opaque reason");

for (const body of ["<html>opaque</html>", "  ", "x".repeat(401)])
  assert.equal(runtimeLoginErrorMessage(new TransportError(502, "/login", body)), generic);
assert.equal(runtimeLoginErrorMessage(null), generic);

assert.equal(runtimeLoginErrorMessage("opaque", "notify"), "Phone notification unavailable. Check your notification channels.");
console.log("runtime login error message: 4 groups passed");
