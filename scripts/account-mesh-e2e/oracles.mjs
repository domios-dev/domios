import { digest } from "./manifest.mjs";

export const SIGNED_OUT_COPY = "Signed out — sign in to use the mesh";
export const MESH_MACHINES_LIMIT_KEY = "mesh.machines";
export const FREE_MACHINE_LIMIT = 2;

const pass = (id, evidence = {}) => ({ id, status: "PASS", reasons: [], evidence });
const fail = (id, reasons, evidence = {}) => ({ id, status: "FAIL", reasons, evidence });
const verdict = (id, reasons, evidence) => (reasons.length ? fail(id, reasons, evidence) : pass(id, evidence));

export function projectPeers(peersJson) {
  if (!peersJson || !Array.isArray(peersJson.paired)) return null;
  const paired = {};
  for (const peer of peersJson.paired) {
    paired[peer.instance_id] = {
      public_key_sha256: peer.public_key ? digest(String(peer.public_key)) : null,
      paired_at: peer.paired_at ?? null,
      access_level: peer.access_level ?? null,
      online: peer.online === true,
      version: peer.version ?? null,
    };
  }
  return {
    paired,
    discovered: Array.isArray(peersJson.discovered) ? peersJson.discovered.length : null,
    incoming: Array.isArray(peersJson.incoming_requests) ? peersJson.incoming_requests.length : null,
  };
}

export function projectAccount(statusJson) {
  if (!statusJson || typeof statusJson !== "object" || !("identity" in statusJson)) return null;
  const uid = statusJson.identity?.uid;
  return { signed_in: typeof uid === "string" && uid.length > 0, uid_sha256: uid ? digest(uid) : null, pending: statusJson.pending === true };
}

export function buildMatches(meshStatus, sha) {
  const build = String(meshStatus?.build ?? "");
  return build.length >= 7 && /^[0-9a-f]+$/.test(build) && sha.startsWith(build);
}

function sameRecord(id, before, after, peerId, reasons) {
  const a = before?.paired?.[peerId];
  const b = after?.paired?.[peerId];
  if (!a) reasons.push(`${id}: peer ${peerId} absent before`);
  if (!b) reasons.push(`${id}: peer ${peerId} absent after (pair record lost)`);
  if (!a || !b) return;
  for (const field of ["public_key_sha256", "paired_at", "access_level"]) {
    if (a[field] !== b[field]) reasons.push(`${id}: ${field} changed ${a[field]} -> ${b[field]} (re-pair or access drift)`);
  }
}

export function recordsPreserved(id, before, after, peerId) {
  const reasons = [];
  sameRecord(id, before, after, peerId, reasons);
  return verdict(id, reasons, { peer: peerId, before: before?.paired?.[peerId] ?? null, after: after?.paired?.[peerId] ?? null });
}

export function pairContinuity(id, before, after, peerId) {
  const reasons = [];
  sameRecord(id, before, after, peerId, reasons);
  if (after?.paired?.[peerId] && !after.paired[peerId].online) reasons.push(`${id}: peer ${peerId} offline after`);
  const added = Object.keys(after?.paired ?? {}).filter((k) => !(k in (before?.paired ?? {})));
  if (added.length) reasons.push(`${id}: new pair records appeared: ${added.join(", ")}`);
  if (after?.incoming) reasons.push(`${id}: ${after.incoming} pending pairing request(s) after`);
  return verdict(id, reasons, { peer: peerId, before: before?.paired?.[peerId] ?? null, after: after?.paired?.[peerId] ?? null, added });
}

export function noNewLink(id, before, after, peerId) {
  const reasons = [];
  if (before?.paired?.[peerId]) reasons.push(`${id}: ${peerId} already linked before the attempt`);
  if (after?.paired?.[peerId]) reasons.push(`${id}: ${peerId} became linked although admission had to refuse`);
  return verdict(id, reasons, { peer: peerId });
}

export function linkFormed(id, before, after, peerId) {
  const reasons = [];
  if (before?.paired?.[peerId]) reasons.push(`${id}: ${peerId} was linked before Add`);
  if (!after?.paired?.[peerId]) reasons.push(`${id}: ${peerId} not linked after Add`);
  else if (!after.paired[peerId].online) reasons.push(`${id}: ${peerId} linked but offline`);
  return verdict(id, reasons, { peer: peerId, after: after?.paired?.[peerId] ?? null });
}

// A probe is a put/get round trip through the real mesh; a timeout is ambiguous, never "blocked".
export function roundTrip(id, probe) {
  const reasons = [];
  if (!probe) return fail(id, [`${id}: no probe recorded`]);
  if (probe.timedOut) reasons.push(`${id}: probe timed out (bounded deadline), outcome unproven`);
  else if (!probe.put_ok) reasons.push(`${id}: put refused: ${probe.put_error ?? "unknown"}`);
  else if (!probe.get_ok) reasons.push(`${id}: get refused: ${probe.get_error ?? "unknown"}`);
  else if (probe.sent_sha256 !== probe.received_sha256) reasons.push(`${id}: body digest mismatch`);
  return verdict(id, reasons, probe);
}

export function trafficBlocked(id, probes) {
  const reasons = [];
  if (!probes?.length) reasons.push(`${id}: no probes recorded`);
  for (const probe of probes ?? []) {
    if (probe.timedOut) reasons.push(`${id}: ${probe.direction} timed out; a hang is not a proven block`);
    else if (probe.put_ok && probe.get_ok) reasons.push(`${id}: ${probe.direction} still delivered while it had to be blocked`);
    else if (!probe.put_error && !probe.get_error) reasons.push(`${id}: ${probe.direction} failed without a typed refusal`);
    else if (/peer_not_paired/.test(`${probe.put_error ?? ""} ${probe.get_error ?? ""}`)) reasons.push(`${id}: ${probe.direction} refused as unpaired; the pairing was removed instead of blocked`);
  }
  return verdict(id, reasons, { probes });
}

export function ledgerDelivered(id, ledger, fetched) {
  const reasons = [];
  const got = new Map((fetched ?? []).map((f) => [f.id, f]));
  for (const entry of ledger ?? []) {
    const f = got.get(entry.id);
    if (!f || !f.get_ok) reasons.push(`${id}: acknowledged message ${entry.seq} (${entry.id}) not retrievable: ${f?.get_error ?? "not fetched"}`);
    else if (f.received_sha256 !== entry.sent_sha256) reasons.push(`${id}: message ${entry.seq} digest changed`);
  }
  if (!ledger?.length) reasons.push(`${id}: empty ledger proves nothing`);
  return verdict(id, reasons, { sent: ledger?.length ?? 0, fetched: fetched?.length ?? 0 });
}

export function signedIn(id, account, uidSha256) {
  const reasons = [];
  if (!account?.signed_in) reasons.push(`${id}: not signed in`);
  else if (account.uid_sha256 !== uidSha256) reasons.push(`${id}: signed in to an unexpected account`);
  return verdict(id, reasons, { signed_in: account?.signed_in ?? null, uid_matches: account?.uid_sha256 === uidSha256 });
}

export function signedOut(id, account) {
  return verdict(id, account?.signed_in === false ? [] : [`${id}: still signed in`], { signed_in: account?.signed_in ?? null });
}

export function uiCopyShown(id, snapshot) {
  const text = String(snapshot ?? "");
  return verdict(id, text.includes(SIGNED_OUT_COPY) ? [] : [`${id}: exact signed-out copy absent from the Browser snapshot`], { snapshot_sha256: digest(text) });
}

export function limitRefusal(id, enroll) {
  const reasons = [];
  if (!enroll) return fail(id, [`${id}: no enroll result`]);
  if (enroll.ok === true) reasons.push(`${id}: server admitted a third Free machine`);
  if (enroll.limit_key !== MESH_MACHINES_LIMIT_KEY) reasons.push(`${id}: refusal names ${enroll.limit_key}, expected ${MESH_MACHINES_LIMIT_KEY}`);
  if (enroll.limit_value !== FREE_MACHINE_LIMIT) reasons.push(`${id}: refusal limit ${enroll.limit_value}, expected ${FREE_MACHINE_LIMIT}`);
  if (typeof enroll.error_code !== "string" || !enroll.error_code) reasons.push(`${id}: refusal carries no typed error code`);
  return verdict(id, reasons, { error_code: enroll.error_code ?? null, limit_key: enroll.limit_key ?? null, limit_value: enroll.limit_value ?? null });
}

export function slotsHeld(id, before, after, expectedUsed) {
  const reasons = [];
  for (const [label, m] of [["before", before], ["after", after]]) {
    if (!m) { reasons.push(`${id}: no ${label} machines result`); continue; }
    if (m.slots_used !== expectedUsed) reasons.push(`${id}: ${label} slots_used ${m.slots_used}, expected ${expectedUsed}`);
    if (m.slots_limit !== FREE_MACHINE_LIMIT) reasons.push(`${id}: ${label} slots_limit ${m.slots_limit}, expected ${FREE_MACHINE_LIMIT}`);
  }
  const ids = (m) => [...(m?.installation_ids ?? [])].sort().join(",");
  if (before && after && ids(before) !== ids(after)) reasons.push(`${id}: server machine set changed across the refusal`);
  return verdict(id, reasons, { before_used: before?.slots_used ?? null, after_used: after?.slots_used ?? null });
}

export function exactlyOnce(id, operationIds, operationId) {
  const count = (operationIds ?? []).filter((x) => x === operationId).length;
  return verdict(id, count === 1 ? [] : [`${id}: ${count} refusal events for operation ${operationId}, expected exactly 1`], { count });
}

export function modalOpenedOnce(id, ui) {
  return verdict(id, ui?.open_count === 1 ? [] : [`${id}: plans modal open_count ${ui?.open_count ?? "missing"}, expected 1`], { open_count: ui?.open_count ?? null });
}

export function machineRemoved(id, machines, installationId, endpoint) {
  const reasons = [];
  if ((machines?.installation_ids ?? []).includes(installationId)) reasons.push(`${id}: revoked installation still listed`);
  if (machines?.slots_used !== FREE_MACHINE_LIMIT - 1) reasons.push(`${id}: slots_used ${machines?.slots_used} after revoke, expected ${FREE_MACHINE_LIMIT - 1}`);
  if (endpoint?.present !== false) reasons.push(`${id}: revoked endpoint still published`);
  return verdict(id, reasons, { slots_used: machines?.slots_used ?? null, endpoint_present: endpoint?.present ?? null });
}

export function disjointAccounts(id, primary, foreign) {
  const a = new Set(primary?.installation_ids ?? []);
  const leaked = (foreign?.installation_ids ?? []).filter((x) => a.has(x));
  const reasons = leaked.length ? [`${id}: foreign account sees ${leaked.length} primary installation(s)`] : [];
  if (!primary || !foreign) reasons.push(`${id}: both account listings are required`);
  return verdict(id, reasons, { leaked: leaked.length });
}

export function noPerMessageBackend(id, before, after, messages) {
  const reasons = [];
  if (!Number.isInteger(before?.connections) || !Number.isInteger(after?.connections)) reasons.push(`${id}: observed packet counters missing`);
  else if (after.connections !== before.connections) reasons.push(`${id}: ${after.connections - before.connections} backend connection(s) during ${messages} P2P messages`);
  return verdict(id, reasons, { before: before?.connections ?? null, after: after?.connections ?? null, messages, source: "observed packets" });
}

export function expiryBoundary(id, membership, justBefore, atExpiry, clock) {
  const reasons = [];
  if (!Number.isInteger(membership?.expires_at_ms)) reasons.push(`${id}: membership expiry unknown`);
  if (clock?.before?.now_ms !== membership?.expires_at_ms - 1) reasons.push(`${id}: isolated clock was not set to expires_at - 1 ms`);
  if (clock?.at?.now_ms !== membership?.expires_at_ms) reasons.push(`${id}: isolated clock was not set to expires_at exactly`);
  if (roundTrip("before", justBefore).status !== "PASS") reasons.push(`${id}: traffic refused before the original expiry`);
  if (trafficBlocked("at", [atExpiry]).status !== "PASS") reasons.push(`${id}: traffic still flowed at the exact expiry`);
  return verdict(id, reasons, { expires_at_ms: membership?.expires_at_ms ?? null });
}

export function cleanupState(id, result, expected) {
  return verdict(id, result?.cleanup === expected ? [] : [`${id}: cleanup ${result?.cleanup ?? "missing"}, expected ${expected}`], { cleanup: result?.cleanup ?? null });
}

export function noBroadcast(id, cliManifest, peers, packets) {
  const reasons = [];
  const mesh = (cliManifest?.subcommands ?? []).find((s) => s.name === "mesh");
  if ((mesh?.subcommands ?? []).some((s) => s.name === "broadcast" || s.name === "discover")) reasons.push(`${id}: release CLI still exposes mesh broadcast/discover`);
  if (peers?.discovered !== 0) reasons.push(`${id}: ${peers?.discovered ?? "unknown"} discovered rows outside account admission`);
  if (packets?.broadcast_packets !== 0) reasons.push(`${id}: ${packets?.broadcast_packets ?? "unknown"} observed discovery broadcast packets`);
  return verdict(id, reasons, { discovered: peers?.discovered ?? null, broadcast_packets: packets?.broadcast_packets ?? null });
}

export function firestoreClosed(id, probe) {
  return verdict(id, probe?.denied === true ? [] : [`${id}: client Firestore read of mesh data was not denied`], { denied: probe?.denied ?? null });
}
