import { randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { authorityTabs, Changes, friend, observe, Peer, ready, receiving, requireThat, retired, root, topology } from "./peers.mjs";

import { digest, loadManifest, qualifyPeer, repo } from "./qualification.mjs";

const directory = dirname(fileURLToPath(import.meta.url));
const [manifestPath, outputPath] = process.argv.slice(2);
const manifest = loadManifest(manifestPath);
const out = resolve(outputPath);
const location = relative(join(repo, "target-check/evals"), out);
requireThat(location && !isAbsolute(location) && !location.startsWith(".."), "Output must be a new directory inside target-check/evals");
requireThat(!existsSync(out), "Output already exists; uncertain actions must never be replayed");
mkdirSync(out);
const result = { schema: 1, outcome: "RUNNING", candidate_sha: manifest.candidate_sha,
  declared_qa_backend: { target: "qa", project_id: manifest.qa_backend.project_id, functions_sha: manifest.qa_backend.functions_sha },
  started_at: new Date().toISOString(), steps: [], actions: [] };
const save = () => writeFileSync(join(out, "results.json"), JSON.stringify(result, null, 2) + "\n");
const x = manifest.friend_x, y = manifest.friend_y, seconds = manifest.deadline_seconds;
const peers = {}, changes = new Changes();
const ids = [x, y];
const safeRoot = tab => tab ? { id: tab.id, session_id: tab.agent_session_id, nonce_sha256: digest(tab.session_binding_nonce ?? "") } : null;
const summary = snapshot => Object.fromEntries(ids.map(id => [id, {
  receiver: friend(snapshot, id), concierge_state: snapshot.runtime.concierges?.[id]?.state,
  dormant: snapshot.runtime.concierges?.[id]?.concierge_dormant,
  binding: snapshot.runtime.bindings?.[id] ?? null, authority_tabs: authorityTabs(snapshot, id).map(tab => tab.id), root: safeRoot(root(snapshot, id)),
}]));
let otherSession, accountRevision;
const session = tab => tab && `${tab.id}:${tab.agent_session_id}:${tab.session_binding_nonce}`;
const absent = (snapshot, tabIds) => tabIds.every(id => !snapshot.tabs.some(tab => tab.id === id));
const readBoth = async () => {
  const [B, C] = await Promise.all([peers.B.snapshot(), peers.C.snapshot()]);
  if (otherSession) requireThat(session(root(B, y)) === otherSession, "Unmoved friend's concierge/session changed");
  if (accountRevision !== undefined) {
    for (const snapshot of [B, C]) {
      if (snapshot.runtime.receiver_status_known) requireThat(snapshot.runtime.revision === accountRevision, "Friend move changed account receiver revision");
    }
  }
  return { B, C };
};
async function qualifyAll() {
  const proofs = await Promise.all(Object.values(peers).map(peer => qualifyPeer(peer, manifest)));
  result.steps.push({ name: "qualify-before-mutation", at: new Date().toISOString(), peers: proofs });
  save();
}
async function checkpoint(name, predicate) {
  const snapshots = await observe(changes, readBoth, predicate, seconds);
  result.steps.push({ name, at: new Date().toISOString(), B: summary(snapshots.B), C: summary(snapshots.C) });
  save();
  return snapshots;
}
async function select(label, operation) {
  await qualifyAll();
  const state = (await peers[label].snapshot()).runtime.friend_receivers?.[x];
  requireThat(state && Number.isSafeInteger(state.revision) && state.revision >= 0, "Friend receiver revision is unknown");
  const action = { kind: "set_friend_receiver", peer: label, friendship_id: x, receiver_operation: operation,
    expected_receiver_revision: state.revision, at: new Date().toISOString(), outcome: "intent_recorded" };
  result.actions.push(action); save();
  await peers[label].runtime({ operation: "set_friend_receiver", friendship_id: x, receiver_operation: operation,
    takeover_confirmed: true, expected_receiver_revision: state.revision });
  action.outcome = "returned_success"; save();
}
const safeEntry = entry => entry && Object.fromEntries(["message_id", "direction", "at", "handling_state", "handling_updated_at",
  "submission", "delivery_status", "delivered_at", "read_at"].filter(key => entry[key] !== undefined).map(key => [key, entry[key]]));
async function exchange(label, friendship, destination, other, phase) {
  await qualifyAll();
  const marker = `qa-per-friend-${randomUUID()}`;
  const body = `Automated QA routing check. Reply with this public marker once: ${marker}`;
  const action = { kind: "send", peer: label, destination, nonreceiver: other, friendship_id: friendship, marker, at: new Date().toISOString(), outcome: "intent_recorded" };
  result.actions.push(action); save();
  const receipt = await peers[label].runtime({ operation: "send", friendship_id: friendship, kind: "question", body });
  action.receipt = Object.fromEntries(["message_id", "status", "reason", "submission"].filter(key => receipt[key] !== undefined).map(key => [key, receipt[key]]));
  save();
  requireThat(/^[a-f0-9]{32}$/.test(receipt.message_id) && receipt.submission !== "unconfirmed" &&
    receipt.status !== "not_delivered", "Send was refused or uncertain; no replay allowed");
  action.message_id = receipt.message_id; action.outcome = "returned_receipt"; save();
  const arm = Date.parse(action.at);
  const read = async () => {
    const [receiver, wrong, sender, snapshots] = await Promise.all([
      peers[destination].runtime({ operation: "history", friendship_id: friendship, limit: 100 }),
      peers[other].runtime({ operation: "history", friendship_id: friendship, limit: 100 }),
      peers[label].runtime({ operation: "history", friendship_id: friendship, limit: 100 }), readBoth(),
    ]);
    requireThat([receiver, wrong, sender].every(page => Array.isArray(page.entries)), "Invalid history schema");
    const matching = page => page.entries.filter(entry => entry.message_id === receipt.message_id && entry.direction === "in");
    const found = matching(receiver), misplaced = matching(wrong);
    requireThat(misplaced.length === 0, "Message landed on the nonreceiver");
    requireThat(found.length <= 1, "Duplicate inbound message");
    const entry = found[0];
    if (entry) requireThat(entry.body === body && entry.at >= arm, "Inbound message identity/arming mismatch");
    const outgoing = sender.entries.find(entry => entry.direction === "out" && entry.message_id === receipt.message_id);
    return { entry, outgoing, snapshots };
  };
  const observation = await observe(changes, read, ({ entry, outgoing, snapshots }) =>
    ["working", "answered", "refused", "no_reply"].includes(entry?.handling_state) &&
    Number(entry.handling_updated_at) >= arm && outgoing?.submission === "accepted" && outgoing.delivery_status === "delivered" &&
    ready(snapshots[destination], friendship), seconds);
  action.outcome = "delivered_and_picked_up";
  result.steps.push({ name: phase, at: new Date().toISOString(), message_id: receipt.message_id,
    receiver: destination, nonreceiver_absent: true, inbound: safeEntry(observation.entry),
    outgoing: safeEntry(observation.outgoing), root: safeRoot(root(observation.snapshots[destination], friendship)) });
  save();
}
async function restartC() {
  await qualifyAll();
  const before = await qualifyPeer(peers.C, manifest);
  peers.C.close();
  const windowsBash = "C:/Program Files/Git/bin/bash.exe";
  const bash = process.platform === "win32" ? windowsBash : "bash";
  result.actions.push({ kind: "cold_restart", peer: "C", before_boot: before.boot_id, at: new Date().toISOString() }); save();
  const code = await new Promise((accept, reject) => {
    const child = spawn(bash, [join(directory, "restart.sh"), resolve(manifestPath), before.boot_id], {
      cwd: repo, windowsHide: true, env: process.env,
      signal: AbortSignal.timeout(240_000), stdio: ["ignore", "pipe", "pipe"],
    });
    let log = "";
    const retain = chunk => {
      log += chunk.toString();
      if (log.length > 4 * 1024 * 1024) child.kill();
      writeFileSync(join(out, "C-restart.log"), log);
    };
    child.stdout.on("data", retain); child.stderr.on("data", retain);
    child.once("error", () => reject(new Error("Supported C restart failed; see private launcher log")));
    child.once("close", accept);
  });
  requireThat(code === 0, `C restart launcher exit ${code}; no retry`);
  peers.C = new Peer("C");
  const after = await qualifyPeer(peers.C, manifest);
  requireThat(after.boot_id !== before.boot_id, "Cold restart did not change daemon boot identity");
  await peers.C.open(changes);
  result.steps.push({ name: "C-cold-restart", before, after, launcher_exit: code }); save();
}

try {
  save();
  for (const label of Object.keys(topology)) {
    peers[label] = new Peer(label);
    const identity = await qualifyPeer(peers[label], manifest);
    result.steps.push({ name: `qualify-${label}`, ...identity }); save();
  }
  await Promise.all(Object.values(peers).map(peer => peer.open(changes)));
  for (const [sender, friendship] of [["A", x], ["D", y]]) {
    const snapshot = await peers[sender].snapshot();
    requireThat(friend(snapshot, friendship)?.receiving_here === true, `${sender}: sender must be the effective receiver for its side`);
  }
  const initial = await checkpoint("initial-account-receiver-B", ({ B, C }) =>
    B.runtime.receiving_here === true && C.runtime.receiving_here === false &&
    receiving(B, x, true, "account") && receiving(C, x, false, "account") &&
    receiving(B, y, true, "account") && receiving(C, y, false, "account") && ready(B, x) && ready(B, y) && retired(C, x));
  requireThat(initial.C.runtime.permissions?.[x]?.allowed === true, "C must already have its own enabled local grant; setup is never copied");
  const originalX = root(initial.B, x);
  const formerBIds = authorityTabs(initial.B, x).map(tab => tab.id);
  otherSession = session(root(initial.B, y));
  accountRevision = initial.B.runtime.revision;
  requireThat(initial.C.runtime.revision === accountRevision, "Account receiver revision disagrees between B/C");
  await select("C", "set");
  const moved = await checkpoint("friend-X-on-C-only", ({ B, C }) => receiving(B, x, false, "friend") && receiving(C, x, true, "friend") &&
    retired(B, x) && absent(B, formerBIds) && ready(C, x) && receiving(B, y, true, "account") && receiving(C, y, false, "account"));
  const formerCIds = authorityTabs(moved.C, x).map(tab => tab.id);
  await exchange("A", x, "C", "B", "X-delivered-on-C");
  await exchange("D", y, "B", "C", "Y-still-delivered-on-B");
  await select("C", "reset");
  await checkpoint("reset-to-account-B", ({ B, C }) => receiving(B, x, true, "account") && receiving(C, x, false, "account") &&
    ready(B, x) && retired(C, x) && absent(C, formerCIds) && root(B, x).agent_session_id !== originalX.agent_session_id);
  await exchange("A", x, "B", "C", "X-delivered-on-B-after-reset");
  await restartC();
  await checkpoint("cold-nonreceiver-has-no-stale-concierge", ({ B, C }) => receiving(C, x, false, "account") && retired(C, x) &&
    absent(C, formerCIds) && authorityTabs(C, y).length === 0 && !C.runtime.bindings?.[y] &&
    receiving(B, x, true, "account") && ready(B, x) && receiving(B, y, true, "account") && ready(B, y));
  for (const label of Object.keys(topology)) await qualifyPeer(peers[label], manifest);
  for (const action of result.actions.filter(action => action.kind === "send")) {
    const history = await peers[action.nonreceiver].runtime({ operation: "history", friendship_id: action.friendship_id, limit: 100 });
    requireThat(Array.isArray(history.entries) && !history.entries.some(entry => entry.direction === "in" && entry.message_id === action.message_id),
      "A message appeared on its original nonreceiver after the routing observation");
  }
  result.outcome = "PASS";
} catch (error) {
  result.outcome = result.actions.length ? "FAIL" : "BLOCKED";
  result.failure = error instanceof Error ? error.message.slice(0, 300) : "Scenario failed";
  result.followup = "Preserve state and intent ledger. Do not replay uncertain sends, auto-reset receiving or start missing peers. Owner reviews any remaining override.";
  process.exitCode = 1;
} finally {
  Object.values(peers).forEach(peer => peer.close());
  result.finished_at = new Date().toISOString(); save();
  console.log(`${result.outcome}: ${join(out, "results.json")}`);
}
