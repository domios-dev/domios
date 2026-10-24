import assert from "node:assert/strict";
import { webGatewayGraph, WEB_NODE_ID, SELF_PEER_KEY } from "./meshProjection";
import type { PeerStatus } from "./useMeshData";

const peer = (id: string, online: boolean, pending = false): PeerStatus => ({ instance_id: id, name: id, ip: "100.100.1.1", api_port: 7686, online, pending, access_level: "full", last_seen: null, relay_protocol_version: 2 });
const graph = webGatewayGraph([peer("online", true), peer("offline", false), peer("unpaired", true, true)]);
assert.deepEqual(graph.nodes, [WEB_NODE_ID, SELF_PEER_KEY, "online", "offline", "unpaired"]);
assert.deepEqual(graph.edges.map(({ machineId, online }) => [machineId, online]), [[SELF_PEER_KEY, true], ["online", true], ["offline", false]]);
assert.deepEqual(webGatewayGraph([]).nodes, [WEB_NODE_ID, SELF_PEER_KEY]);
assert.equal(webGatewayGraph([], false).edges[0].online, false);
console.log("✓ Gateway graph includes Web and real machines; only paired peers have presence edges");
