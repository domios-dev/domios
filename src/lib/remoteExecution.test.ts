import assert from "node:assert/strict";
import {test} from "node:test";
import type {ProviderDescriptor} from "./types";
import {atomicSpawnPreset, isRemoteTaskTab, remoteProvider, remoteSpawnSelection} from "./remoteExecution";
const provider = {id: "renamed-provider", transport: "chat_backend", remote_task: {
  backend: "fixture", mode: "fixture-cloud", displayName: "Fixture Cloud", availability: {status: "available"},
  environments: [{id: "env", label: "Fixture", accountRef: "account"}], followUp: false, cancel: false, applyDiff: false,
}} as ProviderDescriptor;
test("Cloud is an explicit mode of the same provider and Local presets remain local", () => {
  assert.equal(remoteProvider([provider], provider.id), provider);
  assert.equal(remoteProvider([provider], "foreign"), undefined);
  assert.deepEqual(remoteSpawnSelection([provider], provider.id, "env"), {providerId: provider.id, preset: "env", executionMode: "fixture-cloud"});
  assert.deepEqual(atomicSpawnPreset(provider, "local-preset"), {preset: "local-preset"});
  assert.deepEqual(atomicSpawnPreset(provider, "env", "fixture-cloud"), {config: {preset: "env", executionMode: "fixture-cloud"}});
  assert.throws(() => atomicSpawnPreset(provider, "env", "foreign"));
  assert.throws(() => remoteSpawnSelection([provider], provider.id, "foreign"));
  assert.throws(() => remoteSpawnSelection([{...provider, remote_task: {...provider.remote_task!, environments: []}}], provider.id, "env"));
  assert.throws(() => remoteSpawnSelection([{...provider, remote_task: {...provider.remote_task!, availability: {status: "unavailable", reason: "Fixture unavailable"}}}], provider.id, "env"));
});

test("only the persisted exact mode selects a Cloud result view", () => {
  const tab = {backing: {kind: "chat_backend" as const, plugin_id: provider.id, session_id: "opaque-thread"}};
  assert.equal(isRemoteTaskTab(tab, provider.remote_task), false);
  assert.equal(isRemoteTaskTab({...tab, agent_runtime_config: {executionMode: "local"}}, provider.remote_task), false);
  assert.equal(isRemoteTaskTab({...tab, agent_runtime_config: {executionMode: "foreign"}}, provider.remote_task), false);
  assert.equal(isRemoteTaskTab({...tab, agent_runtime_config: {executionMode: "fixture-cloud"}}, provider.remote_task), true);
});
