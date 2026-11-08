import assert from "node:assert/strict";
import type {FetchOpts, FetchResult} from "@codeterm/plugin-sdk";
import {readProfileLimits, type FetchPort} from "../src/limits";

function port(result: FetchResult) {
  const requests: FetchOpts[] = [];
  const fetch: FetchPort = (opts, then) => {requests.push(opts); return then(result);};
  return {fetch, requests};
}
const balance = {kind: "balance", reader: "deepseek-balance", credentialEnv: "DEEPSEEK_API_KEY"} as const;
const body = JSON.stringify({is_available: true, balance_infos: [{currency: "CNY", total_balance: "110.00", granted_balance: "10.00", topped_up_balance: "100.00"}]});

const ok = port({status: 200, body});
const reading = JSON.parse(readProfileLimits(ok.fetch, {profile: "deep-1", source: balance}, 42) as string);
assert.deepEqual(reading.usage_details.balance, {available: true, amounts: [{currency: "CNY", amount: "110.00"}]});
assert.equal(reading.captured_at_ms, 42);
assert.equal(reading.raw_text, "", "the response body is never retained");
assert.equal(ok.requests.length, 1);
const [request] = ok.requests;
assert.equal(request.url, "https://api.deepseek.com/user/balance");
assert.deepEqual(request.profileSecret, {profile: "deep-1", env: "DEEPSEEK_API_KEY", headers: {Authorization: "Bearer {secret}"}});
assert.equal(JSON.stringify(request).includes("sk-"), false, "the request the plugin builds carries no key value");
assert.equal(request.headers?.Authorization, undefined, "the host, not the plugin, fills Authorization");

function failure(result: FetchResult): string {
  const reply = readProfileLimits(port(result).fetch, {profile: "deep-1", source: balance}, 42);
  assert.equal(typeof reply, "object", JSON.stringify(result).slice(0, 80));
  return (reply as {error: string}).error;
}
assert.equal(failure({error: "opaque host error"}), "opaque host error", "the host's own reason reaches the failure");
const refused = failure({status: 401, body: "opaque-response-body"});
assert.match(refused, /\b401\b/, "a refused read names the HTTP status");
assert.equal(refused.includes("opaque-response-body"), false, "a response body never enters the reason");
assert.equal(failure({status: 200}), failure({status: 200, body: "x".repeat(64 * 1024 + 1)}), "an empty and an oversized response are the same failure");
const unexpected = failure({status: 200, body: "not json"});
assert.equal(unexpected.includes("not json"), false, "an unparsable body is never echoed");
for (const shape of [
  {is_available: true, balance_infos: [{currency: "cny", total_balance: "1"}]},
  {is_available: true, balance_infos: [{currency: "USD", total_balance: 5}]},
  {balance_infos: []},
]) {
  assert.equal(failure({status: 200, body: JSON.stringify(shape)}), unexpected, JSON.stringify(shape));
}
for (const source of [
  {kind: "balance", reader: "unknown-balance", credentialEnv: "KEY"},
  {kind: "balance", reader: "constructor", credentialEnv: "KEY"},
  {kind: "unavailable", reason: "Quota has no public endpoint"},
  {kind: "account", provider: "fixture-native"},
] as const) {
  const unused = port({status: 200, body});
  const reply = readProfileLimits(unused.fetch, {profile: "deep-1", source}, 42);
  assert.equal(typeof (reply as {error: string}).error, "string", `${source.kind} sources answer a failure`);
  assert.equal(unused.requests.length, 0, `${source.kind} sources issue no request`);
}
console.log("✓ ACP balance reader: declared reader only, host-filled credential, bounded parse, named failures, no raw body");
