import assert from "node:assert/strict";
import { Window } from "happy-dom";
import { muxMockFetch } from "../../lib/muxMockFetch.testdata";
import { withFailureDeadline } from "../../test/withFailureDeadline";

async function main() {
  const win = new Window({ url: "http://gateway:7685" });
  Object.defineProperty(globalThis, "window", { configurable: true, value: win });
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: win.localStorage });
  const { projectionForPeer, readNeighborProjection } = await import("./neighborProjection");
  const { setRelayTarget, setTokenFor, selfHost, invalidateWebRequestContext } = await import("../../lib/webApi");
  const blackstar = { pair: { tab_id: "remote-concierge", friend_display_name: "Opaque Friend", friend_photo_url: null, paused: false, access: "paid" as const } };
  assert.deepEqual(projectionForPeer("blackstar", "blackstar", blackstar), blackstar);
  assert.deepEqual(projectionForPeer("eight", "blackstar", blackstar), {});
  assert.deepEqual(projectionForPeer(null, "blackstar", blackstar), {});
  console.log("ok - grouping uses only the selected machine's projection");

  setTokenFor(selfHost(), "owner-sentinel");
  setRelayTarget("blackstar", "Blackstar");
  let accept: ((response: Response) => void) | undefined;
  let entered!: () => void;
  const pending = new Promise<void>(resolve => { entered = resolve; });
  let count = 0;
  globalThis.fetch = muxMockFetch(async (input, init) => {
    assert.equal(String(input), "http://gateway:7685/api/mesh/relay-to");
    assert.equal(init?.method, "POST");
    assert.equal(new Headers(init?.headers).get("Authorization"), "Bearer owner-sentinel");
    assert.deepEqual(JSON.parse(String(init?.body)), { target_peer_id: "blackstar", method: "GET", path: "/api/neighbors/sidebar", body: null });
    count++;
    if (count === 2) return new Promise<Response>(resolve => { accept = resolve; entered(); });
    return Response.json({ ok: true, status: 200, data: blackstar });
  });
  assert.deepEqual(await withFailureDeadline(readNeighborProjection(), "relayed projection"), blackstar);
  console.log("ok - projection GET travels through authenticated selected-peer relay");
  const stale = readNeighborProjection();
  await withFailureDeadline(pending, "pending old-peer projection");
  invalidateWebRequestContext();
  setRelayTarget("eight", "Eight");
  accept!(Response.json({ ok: true, status: 200, data: blackstar }));
  await assert.rejects(withFailureDeadline(stale, "stale projection rejection"), (error: unknown) => error instanceof Error && error.name === "AbortError");
  assert.equal(count, 2);
  console.log("ok - a peer switch rejects the old machine's in-flight result");
}

main().catch(error => { console.error(error); process.exitCode = 1; });
