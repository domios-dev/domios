import assert from "node:assert/strict";
import { surfaceTransportTarget } from "./surfaceTransportTarget";

assert.deepEqual(surfaceTransportTarget({ id: "88a2dfa7" }, null), { tabId: "88a2dfa7" });
assert.deepEqual(surfaceTransportTarget({ id: "88a2dfa7" }, "eight"), { tabId: "88a2dfa7", peerId: "eight" });
assert.deepEqual(surfaceTransportTarget({ id: "mesh:eight/88a2dfa7", remote_peer_id: "eight", remote_tab_id: "88a2dfa7" }, "other"), { tabId: "88a2dfa7", peerId: "eight" });
assert.throws(() => surfaceTransportTarget({ id: "mesh:broken" }, null));
assert.throws(() => surfaceTransportTarget({ id: "local", remote_peer_id: "eight", remote_tab_id: "remote" }, null));
assert.throws(() => surfaceTransportTarget({ id: "mesh:eight/tab", remote_peer_id: "other", remote_tab_id: "tab" }, null));
console.info("surfaceTransportTarget: 6 checks passed");
