import assert from "node:assert/strict";
import { providerErrorReason } from "./providerSignals";

assert.equal(providerErrorReason("rate_limit"), "Rate limited");
assert.equal(providerErrorReason("usage_limit"), "Usage limit reached");
assert.equal(providerErrorReason("server_overloaded"), "Provider overloaded");
assert.equal(providerErrorReason("auth_expiry"), "Sign-in expired");
assert.equal(providerErrorReason("authentication"), "Sign in to continue");
assert.equal(providerErrorReason("other"), "Provider error");
assert.equal(providerErrorReason(null), null);
assert.equal(providerErrorReason(undefined), null);
console.log("provider signals: reason labels passed");
