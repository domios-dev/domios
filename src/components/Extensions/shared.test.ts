import { apiBaseOf, type AppConfigShape } from "./shared";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const configA: AppConfigShape = { api: { host: "127.0.0.1", port: 7686, token: "tok-a" } };
const { headers: headersA1 } = apiBaseOf(configA);
const { headers: headersA2 } = apiBaseOf(configA);
assert(headersA1 === headersA2, "apiBaseOf must return the same headers reference for repeat calls with the same config object");

// Stable config values must preserve header identity across parent renders so effect dependencies do not re-fire.
const configA2: AppConfigShape = { api: { host: "127.0.0.1", port: 7686, token: "tok-a" } };
const { headers: headersA3 } = apiBaseOf(configA2);
assert(headersA1 === headersA3, "apiBaseOf must return the same headers reference for a structurally-equal but distinct config object");

const configB: AppConfigShape = { api: { host: "127.0.0.1", port: 7686, token: "tok-b" } };
const { headers: headersB } = apiBaseOf(configB);
assert(headersB !== headersA1, "apiBaseOf must return a new headers reference when the auth token changes");
assert(headersB.Authorization === "Bearer tok-b", "headers must carry the new token's Authorization header");

const { headers: headersNoAuth1 } = apiBaseOf(null);
const { headers: headersNoAuth2 } = apiBaseOf({});
assert(headersNoAuth1 === headersNoAuth2, "apiBaseOf must return the same headers reference across configs with no token");
assert(headersNoAuth1.Authorization === undefined, "headers must omit Authorization when there is no token");

console.log("shared.apiBaseOf tests: 4/4 passed");
