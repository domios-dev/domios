import assert from "node:assert/strict";
import {test} from "node:test";
import {atomicSpawnPreset, remoteProvider, remoteSpawnSelection} from "./remoteExecution";
import type {ProviderDescriptor} from "./types";

const remote = {id: "renamed-remote", transport: "chat_backend", remote_task: {
  backend: "fixture", localProviderId: "renamed-local", availability: {status: "available"},
  environments: [{id: "environment", label: "Fixture environment", accountRef: "account"}],
  followUp: false, cancel: false, applyDiff: false,
}} as ProviderDescriptor;

test("remote selection uses provider capabilities and explicit environment identity", () => {
  assert.equal(remoteProvider([remote], "foreign"), undefined);
  assert.equal(remoteProvider([remote, {...remote, id: "ambiguous"}], "renamed-local"), undefined);
  assert.deepEqual(remoteSpawnSelection([remote], "renamed-local", "environment"), {providerId: remote.id, preset: "environment"});
  assert.throws(() => remoteSpawnSelection([remote], "renamed-local", "foreign"));
  const unavailable = {...remote, remote_task: {...remote.remote_task!, availability: {status: "unavailable" as const, reason: "Fixture account unavailable"}}};
  assert.throws(() => remoteSpawnSelection([unavailable], "renamed-local", "environment"), /Fixture account unavailable/);
  assert.deepEqual(atomicSpawnPreset(remote, "environment"), {config: {preset: "environment"}});
  assert.deepEqual(atomicSpawnPreset(undefined, "local-preset"), {preset: "local-preset"});
});
