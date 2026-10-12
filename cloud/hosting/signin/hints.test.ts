import assert from "node:assert/strict";
import { test } from "node:test";
import { signinHint, hintedAction } from "./hints";
test("known providers and modes parse without accepting unknown or duplicate hints", () => {
  for (const provider of ["google","facebook","email"]) {
    const hint = signinHint(new URLSearchParams({provider,mode:"signup"}));
    assert.equal(hint.provider, provider); assert.equal(hint.mode, "signup");
  }
  assert.deepEqual(signinHint(new URLSearchParams()), {provider:null,mode:"signin"});
  assert.deepEqual(signinHint(new URLSearchParams("provider=apple&mode=invalid")), {provider:null,mode:"signin"});
  assert.deepEqual(signinHint(new URLSearchParams("provider=google&provider=facebook&mode=signup&mode=signin")), {provider:null,mode:"signin"});
});
test("enabled providers start immediately, email opens its form, and email callbacks take priority", () => {
  const social = signinHint(new URLSearchParams("provider=google"));
  assert.equal(hintedAction(social, ["google","facebook"], false), "google");
  assert.equal(hintedAction(social, ["facebook"], false), "choices");
  assert.equal(hintedAction(signinHint(new URLSearchParams("provider=email")), ["google"], false), "email");
  assert.equal(hintedAction(social, ["google"], true), "finish-email");
  assert.equal(hintedAction(signinHint(new URLSearchParams()), ["google","facebook"], false), "choices");
});
