import assert from "node:assert/strict";
import fs from "node:fs";
import type {FetchOpts, FetchResult} from "@codeterm/plugin-sdk";
import {parseStatusLineQuota, readProfileLimits, statusLineQuotaSnapshot, type LimitsPorts} from "../src/limits";

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
assert.equal(runtime.launchProfiles["gemini-cli"], undefined, "obsolete gemini-cli is not bundled in launchProfiles");
assert.equal(manifest.permissions.subprocess.allow.includes("gemini"), false, "unused gemini binary is removed from subprocess allowlist");
assert.deepEqual(runtime.registryDefaults.gemini.limitsSource, quotaSource);
assert.ok(manifest.permissions.network.allow.includes("cloudcode-pa.googleapis.com"), "the reader's host is declared");
const declared = Object.fromEntries(manifest.credentials.map((entry: {id: string}) => [entry.id, entry]));
assert.equal(declared.geminiOauth.file, "~/.gemini/oauth_creds.json");
assert.deepEqual(declared.geminiOauth.secret, {accessToken: "access_token"}, "the token is a host-only secret field");
assert.equal(Object.values(declared.geminiOauth.public).some(path => /token/.test(String(path))), false, "no token is a public field");
assert.deepEqual(declared.geminiSettings.public, {selectedType: "security.auth.selectedType"});
assert.equal(declared.geminiSettings.secret, undefined);
console.log("✓ ACP quota reader: Google sign-in only, host-filled token, per-model parse, named reasons, no raw body or project");

const antigravity = runtime.launchProfiles.antigravity;
assert.equal(antigravity.binary, "agy");
assert.deepEqual(antigravity.args, ["acp"]);
assert.equal(antigravity.icon, "antigravity-color.svg");
assert.deepEqual(antigravity.limitsSource, {kind: "quota", reader: "statusline-quota"});
assert.ok(manifest.permissions.subprocess.allow.includes("agy"), "Antigravity launch binary is allowlisted");

const runtimeFiles = fs.readFileSync(new URL("../runtime-files.txt", import.meta.url), "utf8").split(/\r?\n/).map(line => line.trim()).filter(Boolean);
assert.equal(runtimeFiles.includes("gemini-color.svg"), false, "unused gemini-color.svg is removed from runtime-files.txt");
assert.ok(runtimeFiles.includes(antigravity.icon), "Antigravity icon is registered in runtime-files.txt");
const iconBytes = fs.readFileSync(new URL(`../${antigravity.icon}`, import.meta.url));
assert.ok(iconBytes.length > 0 && iconBytes.length <= 8 * 1024, "Antigravity icon fits host SVG size bound");
const iconSvg = iconBytes.toString("utf8");
assert.match(iconSvg, /^<svg viewBox="0 0 24 24" xmlns="http:\/\/www\.w3\.org\/2000\/svg">/);
assert.equal(/<(mask|filter|script|style|title|foreignObject)\b/i.test(iconSvg), false, "Antigravity icon uses only sanitizer-allowed SVG elements");
const definedIds = new Set([...iconSvg.matchAll(/\bid="([A-Za-z_][A-Za-z0-9_-]*)"/g)].map(match => match[1]));
const referencedIds = [...iconSvg.matchAll(/url\(#([A-Za-z_][A-Za-z0-9_-]*)\)/g)].map(match => match[1]);
assert.ok(definedIds.size > 0 && referencedIds.length > 0, "Antigravity color mark uses local gradients");
for (const id of referencedIds) assert.ok(definedIds.has(id), `local gradient reference #${id} must be defined`);

for (const [id, pkg, args] of [
  ["grok-build", "@xai-official/grok@1.0.93", ["@xai-official/grok@1.0.93", "agent", "stdio"]],
  ["github-copilot-cli", "@github/copilot@1.0.114", ["@github/copilot@1.0.114", "--acp"]],
  ["cline", "cline@3.0.75", ["cline@3.0.75", "--acp"]],
  ["qwen-code", "@qwen-code/qwen-code@0.28.3", ["@qwen-code/qwen-code@0.28.3", "--acp", "--experimental-skills"]],
  ["kimi-code", "@moonshot-ai/kimi-code@2.7.2", ["@moonshot-ai/kimi-code@2.7.2", "acp"]],
  ["kilo", "@kilocode/cli@7.8.32", ["@kilocode/cli@7.8.32", "acp"]],
] as const) {
  const entry = runtime.launchProfiles[id];
  assert.ok(entry, `bundled profile ${id} must be declared`);
  assert.equal(entry.binary, "npx", `${id} uses npx`);
  assert.deepEqual(entry.args, args, `${id} args match registry`);
  assert.deepEqual(entry.install, {package: pkg, registry: "https://registry.npmjs.org"}, `${id} install pins package`);
  assert.equal(entry.icon, `${id}.svg`, `${id} references bundled svg icon`);
  assert.ok(runtimeFiles.includes(entry.icon), `${id} icon is listed in runtime-files.txt`);
  const svgBytes = fs.readFileSync(new URL(`../${entry.icon}`, import.meta.url));
  assert.ok(svgBytes.length > 0 && svgBytes.length <= 8 * 1024, `${id} icon fits host SVG size bound`);
  const svg = svgBytes.toString("utf8");
  assert.match(svg, /^<svg\b[^>]*viewBox="[^"]+"[^>]*>/, `${id} icon has root svg with viewBox`);
  assert.equal(/<(mask|filter|script|style|title|foreignObject)\b|\b(on[a-z]+|href|xlink:href|data-name)=|url\(/i.test(svg), false, `${id} icon is sanitizer-safe`);
}

const runtimeManifest = JSON.parse(fs.readFileSync(new URL("./runtime-manifest.json", import.meta.url), "utf8"));
assert.deepEqual(runtimeManifest.agentRuntime.launchProfiles, runtime.launchProfiles, "runtime-manifest.json stays in sync with plugin.json");

const antigravityPort = port({status: 200, body});
const antigravityReply = readProfileLimits(antigravityPort.ports, {profile: "antigravity", source: antigravity.limitsSource}, NOW);
assert.equal(typeof (antigravityReply as {error: string}).error, "string", "statusline-quota JS fallback names host hook/probe path cleanly");
assert.equal(antigravityPort.requests.length, 0, "statusline-quota issues no direct network fetch from JS");
console.log("✓ ACP Antigravity profile: manifest metadata, allowlisted binary, sanitizer-safe color SVG, statusline-quota limitsSource");

// Extensible statusLine quota JSON parser (real captured agy payload, human labels, primary-first order, disabled, malformed fail-closed)
{
  const SL_NOW = 1_795_280_000_000;
  const realAgyPayload = {
    product: "antigravity",
    plan_tier: "Google AI Ultra",
    email: "owner@example.com",
    quota: {
      "3p-5h": {remaining_fraction: 1, reset_time: "2026-11-22T02:38:44Z", reset_in_seconds: 17999},
      "3p-weekly": {remaining_fraction: 1, reset_time: "2026-11-28T21:38:44Z", reset_in_seconds: 604799},
      "gemini-5h": {remaining_fraction: 0.9533681, reset_time: "2026-11-22T02:31:19Z", reset_in_seconds: 17554},
      "gemini-weekly": {remaining_fraction: 0.51503307, reset_time: "2026-11-28T11:31:19Z", reset_in_seconds: 568354},
    },
  };
  const realParsed = parseStatusLineQuota(realAgyPayload, SL_NOW);
  assert.deepEqual(realParsed, {
    title: "Account quota (Google AI Ultra)",
    limits: [
      {label: "Gemini 5h", used_percent: 4.7, resets_at: Date.parse("2026-11-22T02:31:19Z")},
      {label: "Gemini weekly", used_percent: 48.5, resets_at: Date.parse("2026-11-28T11:31:19Z")},
      {label: "3P 5h", used_percent: 0, resets_at: Date.parse("2026-11-22T02:38:44Z")},
      {label: "3P weekly", used_percent: 0, resets_at: Date.parse("2026-11-28T21:38:44Z")},
    ],
  }, "real agy payload formats clean human labels and orders Gemini 5h/weekly ahead of 3P 5h/weekly");
  const realSnap = statusLineQuotaSnapshot(realAgyPayload, SL_NOW);
  assert.equal(typeof realSnap, "string");
  assert.equal((realSnap as string).includes("owner@example.com"), false, "email is never retained in stored quota snapshot");

  const isoReset = "2026-11-22T07:50:32Z";
  const parsed = parseStatusLineQuota({
    plan_tier: "Pro",
    quota: {
      "gemini-weekly": {remaining_fraction: 0.9378, reset_time: isoReset, reset_in_seconds: 560580},
      "gemini-pro": {remaining_fraction: 0.25, reset_in_seconds: 1800},
      "disabled-group": {remaining_fraction: 0, disabled: true, reset_in_seconds: 60},
      "amount-only": {remaining_amount: 42},
    },
  }, SL_NOW);
  assert.deepEqual(parsed, {
    title: "Account quota (Pro)",
    limits: [
      {label: "Gemini weekly", used_percent: 6.2, resets_at: Date.parse(isoReset)},
      {label: "Gemini Pro", used_percent: 75, resets_at: SL_NOW + 1_800_000},
    ],
  }, "parses remaining_fraction, prefers ISO reset_time over reset_in_seconds, skips disabled and amount-only buckets");

  const fromJsonString = statusLineQuotaSnapshot(JSON.stringify({
    quota: {"claude-5h": {remaining_fraction: 0.5, reset_in_seconds: 0}},
  }), SL_NOW);
  assert.equal(typeof fromJsonString, "string");
  const decoded = JSON.parse(fromJsonString as string);
  assert.equal(decoded.billing_mode, "subscription");
  assert.equal(decoded.limits_reported, true);
  assert.deepEqual(decoded.usage_details.quota, {
    title: "Account quota",
    limits: [{label: "Claude 5h", used_percent: 50, resets_at: null}],
  });

  const allDisabled = statusLineQuotaSnapshot({quota: {"group-a": {remaining_fraction: 0.5, disabled: true}}}, SL_NOW);
  assert.equal(typeof allDisabled, "object");
  assert.match((allDisabled as {error: string}).error, /no active model quota/);

  for (const malformed of [
    null,
    "",
    "not json",
    "x".repeat(64 * 1024 + 1),
    {},
    {quota: []},
    {quota: {"": {remaining_fraction: 0.5}}},
    {quota: {"bad\nkey": {remaining_fraction: 0.5}}},
    {quota: {"group-a": "bad"}},
    {quota: {"group-a": {remaining_fraction: -0.01}}},
    {quota: {"group-a": {remaining_fraction: 1.01}}},
    {quota: {"group-a": {remaining_fraction: Number.NaN}}},
    {quota: {"group-a": {remaining_fraction: "0.5"}}},
    {quota: {"group-a": {remaining_fraction: 0.5, disabled: "true"}}},
    {quota: {"group-a": {remaining_fraction: 0.5, reset_time: "tomorrow"}}},
    {quota: {"group-a": {remaining_fraction: 0.5, reset_in_seconds: -10}}},
    {quota: {"group-a": {remaining_fraction: 0.5, reset_in_seconds: 40 * 86400}}},
    {quota: Object.fromEntries(Array.from({length: 129}, (_, i) => [`bucket-${i}`, {remaining_fraction: 0.5}]))},
  ]) {
    assert.equal(parseStatusLineQuota(malformed, SL_NOW), null, `must reject malformed statusLine quota: ${String(malformed).slice(0, 60)}`);
  }

  const pushReply = readProfileLimits(port({status: 200, body}).ports, {profile: "future-acp", source: {kind: "quota", reader: "statusline-quota"}}, SL_NOW);
  assert.equal(typeof pushReply, "object");
  assert.match((pushReply as {error: string}).error, /agent hook/);

  const limitsSourceText = fs.readFileSync(new URL("../src/limits.ts", import.meta.url), "utf8");
  for (const forbidden of [".at(", "Object.hasOwn(", ".replaceAll(", "console."]) {
    assert.equal(limitsSourceText.includes(forbidden), false, `limits.ts must stay QuickJS ES2020-safe (found ${forbidden})`);
  }
  console.log("✓ ACP statusLine quota parser: fractions, reset_time/reset_in_seconds, disabled, malformed fail-closed, QuickJS ES2020-safe");
}
