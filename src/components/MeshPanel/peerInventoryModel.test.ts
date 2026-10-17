import { strict as assert } from "node:assert";
import { peerInventoryModel } from "./peerInventoryModel";
import type { Service } from "./types";

const peer = { instance_id: "remote", name: "Remote", online: true, services_loaded: true };
const resource = (id: string, name: string, kind: "daemon" | "cron", source_peer_id?: string): Service => ({
  id, name, kind, source_peer_id, health_mode: "http", health_interval: 60,
  health: { status: "healthy" }, enabled: true, push_enabled: false, registered_at: 1, updated_at: 1,
});
const resources = [resource("b", "Zulu", "daemon", "remote"), resource("a", "alpha", "daemon", "remote"), resource("c", "Backup", "cron", "remote"), resource("d", "Local", "daemon"), resource("e", "Foreign", "daemon", "other")];
const model = peerInventoryModel(resources, peer);
assert.deepEqual(model.services.map((row) => row.resource.name), ["alpha", "Zulu"]);
assert.deepEqual(model.crons.map((row) => row.resource.name), ["Backup"]);
assert.equal(model.state, "ready");
assert.deepEqual(peerInventoryModel(resources, peer, true).services.map((row) => row.resource.name), ["Local"]);
assert.equal(peerInventoryModel([], peer).state, "ready");
assert.equal(peerInventoryModel([], { ...peer, online: false }).state, "unavailable");
assert.equal(peerInventoryModel([], { ...peer, pending: true }).state, "unavailable");
assert.equal(peerInventoryModel([], { ...peer, services_loaded: false }).state, "unavailable");
assert.equal(peerInventoryModel([], peer, false, true).state, "loading");
assert.equal(peerInventoryModel([], peer, true, false, "failed").state, "unavailable");
assert.equal(peerInventoryModel(resources, { ...peer, services_stale: true }).services.length, 2);
assert.equal(peerInventoryModel(resources, { ...peer, services_error: "partial" }).state, "unavailable");
assert.equal(peerInventoryModel([{ ...resources[0], provenance: { freshness: "stale", offline: false } }], peer).state, "unavailable");
assert.equal(peerInventoryModel([{ ...resources[0], source_peer_id: "other", source_peer: "Remote" }], peer).services.length, 0);
assert.equal(peerInventoryModel([{ ...resources[0], source_peer_id: undefined, machine_id: "remote" }], peer).services.length, 1);
assert.equal(resources[0].name, "Zulu");
console.log("16 peer inventory model assertions passed");
