import assert from "node:assert/strict";
import { confirmedTabFastMode, tabMetaParts } from "./tabMetaLabel";
import type { ProviderDescriptor, Tab } from "./types";

assert.deepEqual(tabMetaParts("model", true, "worker", "idle"), ["model", "fast", "worker", "idle"]);
for (const fast of [false, undefined]) assert.deepEqual(tabMetaParts("model", fast, "worker", "idle"), ["model", "worker", "idle"]);
const provider = { agent_runtime: { fastMode: true } } as ProviderDescriptor;
const tab = { agent_session_id: "own", session_binding_nonce: "nonce", agent_runtime_config: {
  fastMode: true, confirmedFastMode: { enabled: true, sessionId: "own", bindingNonce: "nonce" },
} } as Tab;
assert.equal(confirmedTabFastMode(tab, provider), true);
assert.equal(confirmedTabFastMode({ ...tab, agent_session_id: "foreign" }, provider), undefined);
assert.equal(confirmedTabFastMode({ ...tab, session_binding_nonce: "new" }, provider), undefined);
assert.equal(confirmedTabFastMode({ ...tab, agent_runtime_config: { fastMode: true } }, provider), undefined);
assert.equal(confirmedTabFastMode(tab, {} as ProviderDescriptor), undefined);
console.log("tab meta labels: 8 assertions passed");
