import * as o from "./oracles.mjs";

const LEDGER_SIZE = 3;
const BURST_SIZE = 10;

async function bothWays(d, a, b, seq, id, checks) {
  checks.push(o.roundTrip(`${id}-a-to-b`, await d.probe(a, b, seq)));
  checks.push(o.roundTrip(`${id}-b-to-a`, await d.probe(b, a, seq)));
}

async function blockedBothWays(d, a, b, seq, id, checks) {
  checks.push(o.trafficBlocked(id, [await d.probe(a, b, seq), await d.probe(b, a, seq)]));
}

async function continuity(d, before, id, checks) {
  const after = { a: await d.peers("paired_a"), b: await d.peers("paired_b") };
  checks.push(o.pairContinuity(`${id}-a`, before.a, after.a, d.targetOf("paired_b").instance_id));
  checks.push(o.pairContinuity(`${id}-b`, before.b, after.b, d.targetOf("paired_a").instance_id));
  return after;
}

async function putLedger(d, from, to) {
  const ledger = [];
  for (let seq = 0; seq < LEDGER_SIZE; seq += 1) {
    const p = await d.probe(from, to, 100 + seq);
    if (p.put_ok) ledger.push({ seq: p.seq, id: p.id, sent_sha256: p.sent_sha256 });
  }
  return ledger;
}

async function fetchLedger(d, from, to, ledger) {
  const fetched = [];
  for (const entry of ledger) fetched.push(await d.fetch(from, to, entry.id));
  return fetched;
}

async function signIn(d, role, accountKey) {
  await d.op("login", role, { account_ref: d.manifest.accounts[accountKey].ref });
  return d.account(role);
}

async function expectBuild(d, role, sha, id, checks) {
  const status = await d.meshStatus(role);
  checks.push({ id, status: o.buildMatches(status, sha) ? "PASS" : "FAIL", reasons: o.buildMatches(status, sha) ? [] : [`${id}: build ${status?.build} is not ${sha}`], evidence: { build: status?.build ?? null } });
}

// Order matters: destructive revoke runs last, and every phase names who restores what it changes.
export const PHASES = Object.freeze([
  {
    id: "release-identity",
    authorization: false,
    requires: ["backend_readback"],
    cleanup_owner: "none (read-only)",
    async run(d, checks) {
      const back = await d.op("backend_readback", "paired_a");
      const { backend_sha, backend_project } = d.manifest.release;
      const reasons = [];
      if (back.sha !== backend_sha) reasons.push(`deployed backend sha ${back.sha} is not ${backend_sha}`);
      if (back.project !== backend_project) reasons.push(`deployed backend project ${back.project} is not ${backend_project}`);
      checks.push({ id: "backend-sha-readback", status: reasons.length ? "FAIL" : "PASS", reasons, evidence: { sha: back.sha ?? null, project: back.project ?? null } });
    },
  },
  {
    id: "upgrade-continuity",
    authorization: true,
    requires: ["upgrade", "restart"],
    cleanup_owner: "harness removes its probe pastes; Turner owns binary rollback of paired_a/paired_b",
    async run(d, checks) {
      const { old_client_sha: oldSha, client_sha: newSha } = d.manifest.release;
      await expectBuild(d, "paired_a", oldSha, "a-starts-old", checks);
      await expectBuild(d, "paired_b", oldSha, "b-starts-old", checks);
      const before = { a: await d.peers("paired_a"), b: await d.peers("paired_b") };
      await bothWays(d, "paired_a", "paired_b", 1, "old-old-traffic", checks);
      const ledgerOnA = await putLedger(d, "paired_b", "paired_a");

      await d.op("upgrade", "paired_b");
      await expectBuild(d, "paired_b", newSha, "b-upgraded", checks);
      await continuity(d, before, "mixed-old-new", checks);
      await bothWays(d, "paired_a", "paired_b", 2, "mixed-traffic", checks);
      checks.push(o.ledgerDelivered("b-upgrade-no-drop", ledgerOnA, await fetchLedger(d, "paired_b", "paired_a", ledgerOnA)));

      await d.op("restart", "paired_b");
      await expectBuild(d, "paired_b", newSha, "b-restarted", checks);
      await continuity(d, before, "after-restart", checks);
      await bothWays(d, "paired_a", "paired_b", 3, "restart-traffic", checks);

      const ledgerOnB = await putLedger(d, "paired_a", "paired_b");
      await d.op("upgrade", "paired_a");
      await expectBuild(d, "paired_a", newSha, "a-upgraded", checks);
      await continuity(d, before, "new-new", checks);
      await bothWays(d, "paired_a", "paired_b", 4, "new-new-traffic", checks);
      checks.push(o.ledgerDelivered("a-upgrade-no-drop", ledgerOnB, await fetchLedger(d, "paired_a", "paired_b", ledgerOnB)));
      for (const e of ledgerOnA) await d.cleanupPaste("paired_b", "paired_a", e.id);
      for (const e of ledgerOnB) await d.cleanupPaste("paired_a", "paired_b", e.id);
    },
  },
  {
    id: "adopt-in-place",
    authorization: true,
    requires: ["login", "enroll", "membership"],
    cleanup_owner: "Turner (adopted membership is the intended end state; no unpair)",
    async run(d, checks) {
      const { primary } = d.manifest.accounts;
      const before = { a: await d.peers("paired_a"), b: await d.peers("paired_b") };
      for (const role of ["paired_a", "paired_b"]) {
        const current = await d.account(role);
        const acct = current?.signed_in ? current : await signIn(d, role, "primary");
        checks.push(o.signedIn(`${role}-primary`, acct, primary.uid_sha256));
        const enroll = await d.op("enroll", role);
        checks.push({ id: `${role}-enrolled`, status: enroll.ok === true ? "PASS" : "FAIL", reasons: enroll.ok === true ? [] : [`${role} enroll refused: ${enroll.error_code}`], evidence: { error_code: enroll.error_code ?? null } });
        await bothWays(d, "paired_a", "paired_b", role === "paired_a" ? 10 : 11, `adoption-${role}-traffic`, checks);
      }
      for (const role of ["paired_a", "paired_b"]) {
        const m = await d.op("membership", role);
        checks.push({ id: `${role}-adopted`, status: m.state === "adopted" ? "PASS" : "FAIL", reasons: m.state === "adopted" ? [] : [`${role} membership state ${m.state}`], evidence: { state: m.state ?? null } });
      }
      await continuity(d, before, "adoption-no-repair", checks);
    },
  },
  {
    id: "signout-block",
    authorization: true,
    requires: ["login", "signout_cleanup", "stream_probe", "ui_snapshot"],
    cleanup_owner: "harness restores the primary sign-in on paired_a through the login hook; Turner if that hook fails",
    async run(d, checks) {
      const { primary, foreign } = d.manifest.accounts;
      const before = { a: await d.peers("paired_a"), b: await d.peers("paired_b") };
      checks.push(o.signedIn("starts-primary", await d.account("paired_a"), primary.uid_sha256));
      const logout = await d.cli("paired_a", ["account", "logout"]);
      checks.push({ id: "logout-answered", status: logout.exit === 0 ? "PASS" : "FAIL", reasons: logout.exit === 0 ? [] : [`account logout exit ${logout.exit}`] });
      checks.push(o.signedOut("signed-out", await d.account("paired_a")));
      checks.push(o.cleanupState("online-cleanup-confirmed", await d.op("signout_cleanup", "paired_a"), "confirmed"));
      checks.push(o.recordsPreserved("signout-keeps-a", before.a, await d.peers("paired_a"), d.targetOf("paired_b").instance_id));
      checks.push(o.recordsPreserved("signout-keeps-b", before.b, await d.peers("paired_b"), d.targetOf("paired_a").instance_id));
      await blockedBothWays(d, "paired_a", "paired_b", 20, "signout-blocks-traffic", checks);
      const stream = await d.op("stream_probe", "paired_a", { peer_instance_id: d.targetOf("paired_b").instance_id });
      checks.push({ id: "signout-blocks-stream", status: stream.delivered === false ? "PASS" : "FAIL", reasons: stream.delivered === false ? [] : [`stream delivered=${stream.delivered}`] });
      checks.push(o.uiCopyShown("signout-ui-copy", (await d.op("ui_snapshot", "paired_a")).text));

      checks.push(o.signedIn("foreign-signed-in", await signIn(d, "paired_a", "foreign"), foreign.uid_sha256));
      await blockedBothWays(d, "paired_a", "paired_b", 21, "foreign-account-stays-blocked", checks);
      checks.push(o.recordsPreserved("foreign-keeps-a", before.a, await d.peers("paired_a"), d.targetOf("paired_b").instance_id));
      await d.cli("paired_a", ["account", "logout"]);

      checks.push(o.signedIn("primary-restored", await signIn(d, "paired_a", "primary"), primary.uid_sha256));
      await continuity(d, before, "same-account-resumes", checks);
      await bothWays(d, "paired_a", "paired_b", 22, "resumed-traffic", checks);
    },
    async cleanup(d) {
      const acct = await d.account("paired_a");
      if (acct?.uid_sha256 !== d.manifest.accounts.primary.uid_sha256) {
        if (acct?.signed_in) await d.cli("paired_a", ["account", "logout"]);
        await signIn(d, "paired_a", "primary");
      }
    },
  },
  {
    id: "add-online-only",
    authorization: true,
    requires: ["login", "enroll", "membership", "machines", "add", "registry_block", "registry_unblock"],
    cleanup_owner: "harness unblocks the registry for add_b; Turner owns the Add fixture link afterwards (never unpaired by the harness)",
    async run(d, checks) {
      const { add } = d.manifest.accounts;
      for (const role of ["add_a", "add_b"]) {
        const current = await d.account(role);
        const acct = current?.uid_sha256 === add.uid_sha256 ? current : await signIn(d, role, "add");
        checks.push(o.signedIn(`${role}-signed-in`, acct, add.uid_sha256));
        const enroll = await d.op("enroll", role);
        checks.push({ id: `${role}-enrolled`, status: enroll.ok === true ? "PASS" : "FAIL", reasons: enroll.ok === true ? [] : [`${role} enroll refused: ${enroll.error_code}`] });
      }
      const bInstallation = (await d.op("membership", "add_b")).installation_id;
      const listed = await d.op("machines", "add_a");
      const seen = (listed.installation_ids ?? []).includes(bInstallation);
      checks.push({ id: "account-lists-unlinked-machine", status: seen ? "PASS" : "FAIL", reasons: seen ? [] : ["add_b absent from add_a's account machine list"] });
      const bId = d.targetOf("add_b").instance_id;
      const aId = d.targetOf("add_a").instance_id;
      const before = { a: await d.peers("add_a"), b: await d.peers("add_b") };

      await d.op("registry_block", "add_b");
      const refused = await d.op("add", "add_a", { peer_installation_id: bInstallation });
      checks.push({ id: "offline-participant-refused", status: refused.ok === false ? "PASS" : "FAIL", reasons: refused.ok === false ? [] : ["Add succeeded although add_b could not obtain a fresh permit"], evidence: { error_code: refused.error_code ?? null } });
      checks.push(o.noNewLink("no-link-a", before.a, await d.peers("add_a"), bId));
      checks.push(o.noNewLink("no-link-b", before.b, await d.peers("add_b"), aId));
      await d.op("registry_unblock", "add_b");

      const added = await d.op("add", "add_a", { peer_installation_id: bInstallation });
      checks.push({ id: "online-add-succeeds", status: added.ok === true ? "PASS" : "FAIL", reasons: added.ok === true ? [] : [`Add refused: ${added.error_code}`] });
      checks.push(o.linkFormed("link-a", before.a, await d.peers("add_a"), bId));
      checks.push(o.linkFormed("link-b", before.b, await d.peers("add_b"), aId));
      await bothWays(d, "add_a", "add_b", 30, "added-direct-traffic", checks);
    },
    async cleanup(d) { await d.op("registry_unblock", "add_b"); },
  },
  {
    id: "backend-offline-expiry",
    authorization: true,
    requires: ["membership", "firebase_block", "firebase_unblock", "firebase_packets", "time_control"],
    cleanup_owner: "harness unblocks Firebase and returns the isolated clock to real time; Turner verifies renewal",
    async run(d, checks) {
      const before = { a: await d.peers("paired_a"), b: await d.peers("paired_b") };
      const membership = await d.op("membership", "paired_a");
      for (const role of ["paired_a", "paired_b"]) await d.op("firebase_block", role);
      const packetsBefore = await d.op("firebase_packets", "paired_a");
      const burst = [];
      for (let seq = 0; seq < BURST_SIZE; seq += 1) burst.push(await d.probe("paired_a", "paired_b", 40 + seq));
      const packetsAfter = await d.op("firebase_packets", "paired_a");
      burst.forEach((p, i) => checks.push(o.roundTrip(`backend-offline-p2p-${i}`, p)));
      checks.push(o.noPerMessageBackend("no-firebase-on-p2p-path", packetsBefore, packetsAfter, BURST_SIZE));

      const clockBefore = await d.op("time_control", "paired_a", { now_ms: membership.expires_at_ms - 1 });
      const justBefore = await d.probe("paired_a", "paired_b", 60);
      const clockAt = await d.op("time_control", "paired_a", { now_ms: membership.expires_at_ms });
      const atExpiry = await d.probe("paired_a", "paired_b", 61);
      checks.push(o.expiryBoundary("exact-original-expiry", membership, justBefore, atExpiry, { before: clockBefore, at: clockAt }));
      checks.push(o.recordsPreserved("expiry-keeps-records", before.a, await d.peers("paired_a"), d.targetOf("paired_b").instance_id));
    },
    async cleanup(d) {
      for (const role of ["paired_a", "paired_b"]) await d.op("firebase_unblock", role);
      await d.op("time_control", "paired_a", { now_ms: "real" });
    },
  },
  {
    id: "isolation",
    authorization: true,
    requires: ["membership", "machines", "endpoint", "firestore_probe", "signout_cleanup", "login", "firebase_block", "firebase_unblock", "discovery_packets"],
    cleanup_owner: "harness unblocks Firebase on paired_b and restores its primary sign-in; Turner if either hook fails",
    async run(d, checks) {
      const bInstallation = (await d.op("membership", "paired_b")).installation_id;
      checks.push(o.disjointAccounts("callable-account-isolation", await d.op("machines", "paired_a"), await d.op("machines", "add_a")));
      const same = await d.op("endpoint", "paired_a", { peer_installation_id: bInstallation });
      checks.push({ id: "same-account-endpoint-visible", status: same.present === true ? "PASS" : "FAIL", reasons: same.present === true ? [] : ["same-account endpoint not readable"] });
      const foreign = await d.op("endpoint", "add_a", { peer_installation_id: bInstallation });
      checks.push({ id: "foreign-account-endpoint-hidden", status: foreign.present === false ? "PASS" : "FAIL", reasons: foreign.present === false ? [] : ["foreign account reads a primary endpoint"] });
      checks.push(o.firestoreClosed("firestore-closed", await d.op("firestore_probe", "paired_a")));

      await d.op("firebase_block", "paired_b");
      await d.cli("paired_b", ["account", "logout"]);
      checks.push(o.cleanupState("offline-cleanup-pending", await d.op("signout_cleanup", "paired_b"), "pending"));
      await blockedBothWays(d, "paired_a", "paired_b", 70, "pending-cleanup-still-blocks", checks);
      await d.op("firebase_unblock", "paired_b");
      checks.push(o.signedIn("b-restored", await signIn(d, "paired_b", "primary"), d.manifest.accounts.primary.uid_sha256));
      await bothWays(d, "paired_a", "paired_b", 71, "b-resumed", checks);

      checks.push(o.noBroadcast("no-discovery-broadcast", await d.cliManifest("paired_a"), await d.peers("paired_a"), await d.op("discovery_packets", "paired_a")));
    },
    async cleanup(d) {
      await d.op("firebase_unblock", "paired_b");
      const acct = await d.account("paired_b");
      if (acct?.uid_sha256 !== d.manifest.accounts.primary.uid_sha256) await signIn(d, "paired_b", "primary");
    },
  },
  {
    id: "third-refused-revoke",
    authorization: true,
    requires: ["login", "enroll", "membership", "machines", "refusal_events", "ui_plans_modal", "revoke", "endpoint"],
    cleanup_owner: "Turner (revoke of paired_b is destructive by design; re-enroll or restore the fixture)",
    async run(d, checks) {
      const { primary } = d.manifest.accounts;
      checks.push(o.signedIn("third-sign-in-succeeds", await signIn(d, "third", "primary"), primary.uid_sha256));
      const slotsBefore = await d.op("machines", "paired_a");
      const refused = await d.op("enroll", "third");
      checks.push(o.limitRefusal("server-refuses-third", refused));
      checks.push(o.signedIn("refusal-keeps-sign-in", await d.account("third"), primary.uid_sha256));
      checks.push(o.exactlyOnce("one-refusal-event", (await d.op("refusal_events", "third")).operation_ids, refused.operation_id));
      checks.push(o.modalOpenedOnce("plans-modal-once", await d.op("ui_plans_modal", "third")));
      checks.push(o.slotsHeld("slots-remain-two", slotsBefore, await d.op("machines", "paired_a"), o.FREE_MACHINE_LIMIT));
      const third = await d.peers("third");
      for (const role of ["paired_a", "paired_b"]) checks.push(o.noNewLink(`third-not-linked-${role}`, { paired: {} }, third, d.targetOf(role).instance_id));

      const bInstallation = (await d.op("membership", "paired_b")).installation_id;
      const revoke = await d.op("revoke", "paired_a", { peer_installation_id: bInstallation });
      checks.push({ id: "revoke-answered", status: revoke.ok === true ? "PASS" : "FAIL", reasons: revoke.ok === true ? [] : ["revoke refused"] });
      checks.push(o.machineRemoved("revoke-frees-slot", await d.op("machines", "paired_a"), bInstallation, await d.op("endpoint", "paired_a", { peer_installation_id: bInstallation })));
      const admitted = await d.op("enroll", "third");
      checks.push({ id: "third-admitted-after-revoke", status: admitted.ok === true ? "PASS" : "FAIL", reasons: admitted.ok === true ? [] : [`enroll still refused: ${admitted.error_code}`] });
    },
  },
]);

export function phaseReadiness(manifest, phase, { receiptDigest }) {
  const missing = [];
  for (const key of phase.requires) if (!manifest.operations[key]) missing.push(`operation ${key} (accepted receipt naming its argv)`);
  if (phase.authorization) {
    const auth = manifest.authorizations[phase.id];
    if (!auth) missing.push(`authorization receipt for ${phase.id} from Turner`);
    else {
      const actual = receiptDigest(auth.receipt);
      if (actual === null) missing.push(`authorization receipt file ${auth.receipt} unreadable`);
      else if (actual !== auth.sha256) missing.push(`authorization receipt ${auth.receipt} digest ${actual} is not ${auth.sha256}`);
    }
  }
  return missing;
}
