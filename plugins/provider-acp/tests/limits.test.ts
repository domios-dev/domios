import assert from "node:assert/strict";
import fs from "node:fs";
import type {FetchOpts, FetchResult} from "@codeterm/plugin-sdk";
import {readProfileLimits, type LimitsPorts} from "../src/limits";

function port(results: FetchResult | FetchResult[], credentials: Record<string, Record<string, string>> = {}) {
  const queue = Array.isArray(results) ? [...results] : [results];
  const requests: FetchOpts[] = [];
  const ports: LimitsPorts = {
    fetch: (opts, then) => {requests.push(opts); return then(queue.length > 1 ? queue.shift()! : queue[0]);},
    credentialPublic: (id, then) => then(credentials[id] ?? null),
  };
  return {ports, requests};
}
const balance = {kind: "balance", reader: "deepseek-balance", credentialEnv: "DEEPSEEK_API_KEY"} as const;
const body = JSON.stringify({is_available: true, balance_infos: [{currency: "CNY", total_balance: "110.00", granted_balance: "10.00", topped_up_balance: "100.00"}]});

const ok = port({status: 200, body});
const reading = JSON.parse(readProfileLimits(ok.ports, {profile: "deep-1", source: balance}, 42) as string);
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
  const reply = readProfileLimits(port(result).ports, {profile: "deep-1", source: balance}, 42);
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
  {kind: "quota", reader: "unknown-quota"},
  {kind: "quota", reader: "constructor"},
  {kind: "unavailable", reason: "Quota has no public endpoint"},
  {kind: "account", provider: "fixture-native"},
] as const) {
  const unused = port({status: 200, body});
  const reply = readProfileLimits(unused.ports, {profile: "deep-1", source}, 42);
  assert.equal(typeof (reply as {error: string}).error, "string", `${source.kind} sources answer a failure`);
  assert.equal(unused.requests.length, 0, `${source.kind} sources issue no request`);
}
console.log("✓ ACP balance reader: declared reader only, host-filled credential, bounded parse, named failures, no raw body");

const quotaSource = {kind: "quota", reader: "gemini-code-assist"} as const;
const NOW = 1_000_000;
const googleSignIn = {geminiSettings: {selectedType: "oauth-personal"}, geminiOauth: {expiresAt: String(NOW + 60_000)}};
const loaded: FetchResult = {status: 200, body: JSON.stringify({cloudaicompanionProject: "opaque-project", currentTier: {id: "free-tier"}})};
const quota: FetchResult = {status: 200, body: JSON.stringify({buckets: [
  {modelId: "gemini-lite", tokenType: "REQUESTS", remainingFraction: 0.97, resetTime: "2026-11-09T00:00:00Z"},
  {modelId: "gemini-pro", tokenType: "REQUESTS", remainingFraction: 0.875, resetTime: "2026-11-09T00:00:00Z"},
  {modelId: "gemini-pro", tokenType: "TOKENS", remainingFraction: 0.5, remainingAmount: "500", resetTime: "2026-11-09T06:00:00Z"},
  {modelId: "gemini-no-fraction", tokenType: "REQUESTS"},
  {tokenType: "REQUESTS", remainingFraction: 0.1},
]})};

const read = port([loaded, quota], googleSignIn);
const stored = JSON.parse(readProfileLimits(read.ports, {profile: "gemini-cli", source: quotaSource}, NOW) as string);
assert.deepEqual(stored.usage_details.quota.limits, [
  {label: "gemini-pro", used_percent: 50, resets_at: Date.parse("2026-11-09T06:00:00Z")},
  {label: "gemini-lite", used_percent: 3, resets_at: Date.parse("2026-11-09T00:00:00Z")},
], "one row per model at its lowest remaining fraction, closest to the limit first, and buckets without a fraction are skipped");
assert.equal(typeof stored.usage_details.quota.title, "string");
assert.equal(stored.raw_text, "", "the response body is never retained");
assert.equal(JSON.stringify(stored).includes("opaque-project"), false, "the account's project never enters the stored reading");
assert.deepEqual(read.requests.map(({url, method}) => [url, method]), [
  ["https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist", "POST"],
  ["https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota", "POST"],
]);
assert.deepEqual(JSON.parse(read.requests[1].body!), {project: "opaque-project"});
for (const sent of read.requests) {
  assert.deepEqual(sent.credential, {id: "geminiOauth", headers: {Authorization: "Bearer {accessToken}"}}, "the host, not the plugin, fills the sign-in token");
  assert.equal(sent.headers?.Authorization, undefined);
}

function quotaFailure(results: FetchResult | FetchResult[], credentials: Record<string, Record<string, string>> = googleSignIn) {
  const attempt = port(results, credentials);
  const reply = readProfileLimits(attempt.ports, {profile: "gemini-cli", source: quotaSource}, NOW);
  assert.equal(typeof reply, "object", JSON.stringify(results).slice(0, 80));
  return {error: (reply as {error: string}).error, requests: attempt.requests.length};
}

for (const [mode, expected] of [
  [{selectedType: "gemini-api-key"}, /API-key quota has no public endpoint.*Google AI Studio/],
  [{selectedType: "vertex-ai"}, /Vertex AI/],
  [{selectedType: "opaque-future-mode"}, /sign-in mode/],
] as const) {
  const reply = quotaFailure({status: 200, body: "{}"}, {geminiSettings: mode, geminiOauth: googleSignIn.geminiOauth});
  assert.match(reply.error, expected);
  assert.equal(reply.requests, 0, "a tab not signed in with Google never reads a leftover login's quota");
  assert.equal(reply.error.includes("opaque-future-mode"), false, "an unknown settings value is not echoed");
}
assert.equal(quotaFailure({status: 200, body: "{}"}, {}).requests, 0, "an unreadable sign-in mode reads nothing");
const noToken = quotaFailure({status: 200, body: "{}"}, {geminiSettings: googleSignIn.geminiSettings});
assert.equal(noToken.requests, 0, "a sign-in kept outside a readable file reads nothing");
const expired = quotaFailure({status: 200, body: "{}"}, {geminiSettings: googleSignIn.geminiSettings, geminiOauth: {expiresAt: String(NOW)}});
assert.equal(expired.requests, 0, "an expired token is never sent");
assert.match(expired.error, /expired/);

const unauthorized = quotaFailure({status: 401, body: "opaque-response-body"});
assert.match(unauthorized.error, /\b401\b/);
assert.equal(unauthorized.error.includes("opaque-response-body"), false);
assert.match(quotaFailure([loaded, {status: 403, body: "opaque-response-body"}]).error, /\b403\b/, "a refused quota read names the status");
assert.equal(quotaFailure({error: "opaque host error"}).error, "opaque host error");
const noProject = quotaFailure({status: 200, body: JSON.stringify({currentTier: {id: "standard-tier"}})});
assert.equal(noProject.requests, 1, "without a project no quota request is sent");
assert.match(noProject.error, /GOOGLE_CLOUD_PROJECT/);
for (const shape of [
  {buckets: "none"},
  {buckets: [{modelId: "gemini-pro", remainingFraction: 1.5}]},
  {buckets: [{modelId: "gemini-pro", remainingFraction: "0.5"}]},
  {buckets: [{modelId: "line\nbreak", remainingFraction: 0.5}]},
  {buckets: Array.from({length: 129}, () => ({modelId: "gemini-pro", remainingFraction: 0.5}))},
]) {
  const reply = quotaFailure([loaded, {status: 200, body: JSON.stringify(shape)}]);
  assert.match(reply.error, /unexpected format/, JSON.stringify(shape).slice(0, 80));
}
assert.match(quotaFailure([loaded, {status: 200, body: JSON.stringify({buckets: []})}]).error, /no model quota/, "an account with no buckets is said plainly, not guessed");

const manifest = JSON.parse(fs.readFileSync(new URL("../plugin.json", import.meta.url), "utf8"));
const runtime = manifest.capabilities.agentRuntime;
assert.deepEqual(runtime.launchProfiles["gemini-cli"].limitsSource, quotaSource);
assert.deepEqual(runtime.registryDefaults.gemini.limitsSource, quotaSource);
assert.ok(manifest.permissions.network.allow.includes("cloudcode-pa.googleapis.com"), "the reader's host is declared");
const declared = Object.fromEntries(manifest.credentials.map((entry: {id: string}) => [entry.id, entry]));
assert.equal(declared.geminiOauth.file, "~/.gemini/oauth_creds.json");
assert.deepEqual(declared.geminiOauth.secret, {accessToken: "access_token"}, "the token is a host-only secret field");
assert.equal(Object.values(declared.geminiOauth.public).some(path => /token/.test(String(path))), false, "no token is a public field");
assert.deepEqual(declared.geminiSettings.public, {selectedType: "security.auth.selectedType"});
assert.equal(declared.geminiSettings.secret, undefined);
console.log("✓ ACP quota reader: Google sign-in only, host-filled token, per-model parse, named reasons, no raw body or project");
