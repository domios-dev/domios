// WHAT: contract tests for the manual-tier Account Mesh harness (scripts/account-mesh-e2e.mjs).
// WHY: the harness must refuse wrong targets/SHAs before effects, hold on missing receipts and fail its oracles on real defects.
// ORACLE: exit code, classification, the exact commands the injected runner received, and each oracle's PASS/FAIL on fixtures.
// The fleet below is a structural fake at the process seam; its results are never real-machine evidence.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { main, parseArgs } from "../account-mesh-e2e.mjs";
import { CLASSIFICATION, classify, redact } from "../account-mesh-e2e/journal.mjs";
import { MANIFEST_SCHEMA, validateManifest } from "../account-mesh-e2e/manifest.mjs";
import * as o from "../account-mesh-e2e/oracles.mjs";
import { cliCommandExists, extractJson, runBounded } from "../account-mesh-e2e/runner.mjs";

const repo = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const sha = (s) => createHash("sha256").update(s).digest("hex");
const OLD = "1".repeat(40);
const NEW = "2".repeat(40);
const BACKEND = "3".repeat(40);
const UIDS = { "acct-primary": "uid-primary", "acct-foreign": "uid-foreign", "acct-add": "uid-add" };
const uuid = (n) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const CLI = process.platform === "win32" ? "C:/fixture/codeterm.exe" : "/fixture/codeterm";
const HOOK = process.platform === "win32" ? "C:/hooks" : "/hooks";

function fixture({ operations = {}, authorizations = {}, mutate } = {}) {
  const out = mkdtempSync(join(tmpdir(), "amesh-out-"));
  const target = (n, machine, account, start) => ({ machine, platform: "windows", transport: { kind: "local" }, cli: CLI, api_port: 7700 + n, instance: `fixture-${n}`, instance_id: uuid(n), account, start });
  const manifest = {
    schema: MANIFEST_SCHEMA,
    run_id: "fixture-run",
    evidence_dir: out,
    release: { client_sha: NEW, client_version: "1.16.0", old_client_sha: OLD, old_client_version: "1.15.1", backend_sha: BACKEND, backend_project: "fixture-project" },
    forbidden: { api_ports: [7685], instance_ids: [uuid(99)] },
    accounts: {
      primary: { ref: "acct-primary", uid_sha256: sha("uid-primary"), plan: "free" },
      foreign: { ref: "acct-foreign", uid_sha256: sha("uid-foreign"), plan: "free" },
      add: { ref: "acct-add", uid_sha256: sha("uid-add"), plan: "free" },
    },
    targets: {
      A: target(1, "machine-a", "primary", "old"),
      B: target(2, "machine-b", "primary", "old"),
      C: target(3, "machine-c", "primary", "new"),
      D: target(4, "machine-a", "add", "new"),
      E: target(5, "machine-b", "add", "new"),
    },
    roles: { paired_a: "A", paired_b: "B", third: "C", add_a: "D", add_b: "E" },
    operations,
    authorizations,
    deadlines: { command_ms: 5000, phase_ms: 60000, run_ms: 120000 },
  };
  mutate?.(manifest);
  const path = join(mkdtempSync(join(tmpdir(), "amesh-manifest-")), "manifest.json");
  writeFileSync(path, JSON.stringify(manifest));
  return { manifest, path, out };
}

const SIGNOUT_OPS = {
  login: { argv: [`${HOOK}/login`, "{target}", "{account_ref}"], receipt: "turner-login-hook-r1", fields: {} },
  signout_cleanup: { argv: ["account", "mesh-cleanup"], receipt: "task_0rimhx-cleanup-dto", fields: { cleanup: "cleanup" } },
  stream_probe: { argv: ["mesh", "stream-probe", "{peer_instance_id}"], receipt: "task_s0j8cr-stream-probe", fields: { delivered: "delivered" } },
  ui_snapshot: { argv: [`${HOOK}/ui-text`, "{target}"], receipt: "turner-browser-text-r1", fields: { text: "text" } },
};

function receipt(phase) {
  const path = join(mkdtempSync(join(tmpdir(), "amesh-receipt-")), `${phase}.txt`);
  writeFileSync(path, `GO ${phase}`);
  return { [phase]: { receipt: path, sha256: sha(`GO ${phase}`), granted_by: "Turner" } };
}

// Simulated installations answering the real CLI's JSON shapes; `gate` decides whether mesh traffic flows.
function fleet(manifest, { gate = (from, to) => from.uid === "uid-primary" && to.uid === "uid-primary", wrongInstance, wrongBuild } = {}) {
  const calls = [];
  const byPort = {};
  const byInstance = {};
  for (const [id, t] of Object.entries(manifest.targets)) {
    const s = { id, t, build: (t.start === "old" ? OLD : NEW).slice(0, 9), uid: t.account === "primary" && t.start === "old" ? "uid-primary" : null, signedOut: false, peers: {}, pastes: {} };
    byPort[t.api_port] = s;
    byInstance[t.instance_id] = s;
  }
  const link = (a, b) => {
    a.peers[b.t.instance_id] = { instance_id: b.t.instance_id, public_key: `pk-${b.id}`, paired_at: 1000, access_level: "full", online: true, version: "x", addresses: ["10.0.0.1"] };
  };
  const A = byPort[manifest.targets.A.api_port];
  const B = byPort[manifest.targets.B.api_port];
  link(A, B);
  link(B, A);
  const ok = (json) => ({ exit: 0, stdout: JSON.stringify(json), stderr: "", json, timedOut: false, spawnError: false, ms: 1 });
  const cliOk = (json) => ok({result: json});
  const err = (code) => ({ exit: 1, stdout: JSON.stringify({ ok: false, error: { code, message: code } }), stderr: "", json: { ok: false, error: { code, message: code } }, timedOut: false, spawnError: false, ms: 1 });
  const tree = { name: "codeterm", subcommands: [
    { name: "identify", subcommands: [] }, { name: "cli-manifest", subcommands: [] },
    { name: "mesh", subcommands: ["status", "peers", "stream-probe"].map((name) => ({ name, subcommands: [], args: name === "stream-probe" ? [{ positional: true }] : [] })) },
    { name: "account", subcommands: ["status", "logout", "mesh-cleanup"].map((name) => ({ name, subcommands: [] })) },
    { name: "paste", subcommands: ["put", "get", "rm"].map((name) => ({ name, subcommands: [], args: [{ positional: true }] })) },
  ] };
  let pasteSeq = 0;
  const run = async (argv, { env, input } = {}) => {
    calls.push(argv);
    if (argv[0] === "git") return ok({});
    if (argv[0] === `${HOOK}/login`) {
      const s = Object.values(byPort).find((x) => x.id === argv[1]);
      s.uid = UIDS[argv[2]];
      s.signedOut = false;
      return ok({ ok: true });
    }
    if (argv[0] === `${HOOK}/ui-text`) {
      const s = Object.values(byPort).find((x) => x.id === argv[1]);
      return ok({ text: s.signedOut || !s.uid ? `Mesh\n${o.SIGNED_OUT_COPY}\n` : "Mesh\nMachines" });
    }
    const s = byPort[Number(env.CODETERM_API_PORT)];
    const args = argv.slice(1);
    const cmd = args.filter((a) => !a.startsWith("--")).slice(0, 2).join(" ");
    const meshIdx = args.indexOf("--mesh");
    const peer = meshIdx >= 0 ? byInstance[args[meshIdx + 1]] : null;
    switch (cmd) {
      case "identify": return cliOk({ connection: { port: s.t.api_port, instance: s.t.instance ?? null } });
      case "mesh status": return cliOk({ instance_id: wrongInstance === s.id ? uuid(77) : s.t.instance_id, api_port: s.t.api_port, build: wrongBuild === s.id ? "abcdef123" : s.build });
      case "mesh peers": return cliOk({ paired: Object.values(s.peers), discovered: [], incoming_requests: [] });
      case "cli-manifest": return cliOk(tree);
      case "account status": return cliOk({ identity: s.uid ? { uid: s.uid, email: "x@y" } : null, pending: false, provider: "firebase", session_only: false });
      case "account logout": s.uid = null; s.signedOut = true; return cliOk({ ok: true });
      case "account mesh-cleanup": return cliOk({ cleanup: "confirmed" });
      case "mesh stream-probe": return cliOk({ delivered: gate(s, byInstance[args[2]]) });
      case `paste put`: {
        if (!peer || !s.peers[peer.t.instance_id] || !peer.peers[s.t.instance_id]) return err("peer_not_paired");
        if (!gate(s, peer)) return err("mesh_signed_out");
        pasteSeq += 1;
        peer.pastes[`paste:${pasteSeq}`] = input;
        return cliOk({ id: `paste:${pasteSeq}` });
      }
      default:
        if (cmd.startsWith("paste get")) {
          if (!gate(s, peer)) return err("mesh_signed_out");
          const body = peer.pastes[args[2]];
          return body === undefined ? err("not_found") : cliOk({ id: args[2], body });
        }
        if (cmd.startsWith("paste rm")) return cliOk({ ok: true });
        return err("unknown_command");
    }
  };
  return { run, calls, byPort };
}

const MUTATING = (argv) => argv.includes("logout") || argv.includes("put") || argv.some((a) => a.startsWith(HOOK));
const quiet = () => {};

test("a valid manifest passes and every refusal names its field", () => {
  const { manifest, out } = fixture({ operations: SIGNOUT_OPS });
  assert.deepEqual(validateManifest(manifest, { outDir: out }).errors, []);
  const cases = [
    [(m) => { m.targets.A.api_port = 7685; }, /forbidden \(owner\/prod\) port/],
    [(m) => { m.targets.B.instance_id = uuid(99); }, /owner instance/],
    [(m) => { m.targets.C.instance_id = uuid(1); }, /duplicates targets.A/],
    [(m) => { m.schema = "other"; }, /schema/],
    [(m) => { m.extra = 1; }, /manifest.extra: unknown field/],
    [(m) => { m.release.client_sha = OLD; }, /must differ/],
    [(m) => { m.targets.B.machine = "machine-a"; }, /different real machines/],
    [(m) => { m.accounts.foreign.uid_sha256 = m.accounts.add.uid_sha256; }, /three different accounts/],
    [(m) => { m.targets.A.cli = "codeterm"; }, /absolute path/],
    [(m) => { delete m.targets.D.instance; }, /local port 7704 needs the named instance/],
    [(m) => { m.targets.A.start = "new"; }, /shipped old release/],
    [(m) => { m.operations.enroll = { argv: ["mesh", "disconnect", "x"], receipt: "receipt-x1", fields: { ok: "ok", error_code: "e", limit_key: "k", limit_value: "v", operation_id: "i" } }; }, /forbidden command mesh disconnect/],
    [(m) => { m.operations.stream_probe.argv.push("--mesh"); }, /forbidden token --mesh/],
    [(m) => { m.operations.time_control = { argv: [`${HOOK}/w32tm`, "w32tm"], receipt: "receipt-t1", fields: { now_ms: "now" } }; }, /forbidden token w32tm[\s\S]*isolated_clock/],
    [(m) => { m.operations.ui_snapshot.argv.push("{secret}"); }, /unknown placeholder/],
    [(m) => { m.operations.login.argv[0] = "login"; }, /absolute Turner-owned command/],
    [(m) => { m.operations.signout_cleanup.fields = {}; }, /fields.cleanup: required/],
  ];
  for (const [mutate, pattern] of cases) {
    const copy = structuredClone(manifest);
    mutate(copy);
    const { ok, errors } = validateManifest(copy, { outDir: out });
    assert.equal(ok, false, String(pattern));
    assert.match(errors.join("\n"), pattern);
  }
  assert.match(validateManifest(manifest, { outDir: "/elsewhere" }).errors.join(), /--out is \/elsewhere/);
});

test("an invalid manifest is refused before any command runs or the evidence directory is touched", async () => {
  const { path, out } = fixture({ mutate: (m) => { m.targets.C.api_port = 7685; } });
  const calls = [];
  const code = await main(["--manifest", path, "--out", out], { run: async (argv) => { calls.push(argv); return {}; }, log: quiet });
  assert.equal(code, 2);
  assert.deepEqual(calls, []);
  assert.deepEqual(readdirSync(out), []);
});

test("argument parsing refuses relative paths, unknown phases and mixed modes", () => {
  assert.match(parseArgs(["--manifest", "m.json", "--out", "o"]).errors.join(), /ABSOLUTE_PATH[\s\S]*ABSOLUTE_DIR/);
  assert.match(parseArgs(["--manifest", "/m", "--out", "/o", "--phase", "nope"]).errors.join(), /--phase must be one of/);
  assert.match(parseArgs(["--manifest", "/m", "--out", "/o", "--phase", "signout-block", "--preflight-only"]).errors.join(), /exclusive/);
});

test("preflight-only reads every target and performs no mutation", async () => {
  const { manifest, path, out } = fixture();
  const f = fleet(manifest);
  const code = await main(["--manifest", path, "--out", out, "--preflight-only"], { run: f.run, log: quiet, env: { CODETERM_API_PORT: "7685", CODETERM_API_TOKEN: "owner" } });
  assert.equal(code, 0);
  assert.deepEqual(f.calls.filter(MUTATING), []);
  const results = JSON.parse(readFileSync(join(out, readdirSync(out).find((n) => n.endsWith(".json") && n.startsWith("results-"))), "utf8"));
  assert.equal(results.classification, CLASSIFICATION.PREFLIGHT_PASS);
  assert.ok(results.preflight.every((c) => c.status === "PASS"));
});

test("a target answering with the wrong instance or build is refused before effects", async () => {
  for (const sabotage of [{ wrongInstance: "B" }, { wrongBuild: "C" }]) {
    const { manifest, path, out } = fixture({ operations: SIGNOUT_OPS, authorizations: receipt("signout-block") });
    const f = fleet(manifest, sabotage);
    const code = await main(["--manifest", path, "--out", out], { run: f.run, log: quiet });
    assert.equal(code, 2, JSON.stringify(sabotage));
    assert.deepEqual(f.calls.filter(MUTATING), []);
  }
});

test("missing operations or a tampered authorization receipt hold the phase without effects", async () => {
  const { manifest, path, out } = fixture({ operations: SIGNOUT_OPS, mutate: (m) => { m.authorizations = receipt("signout-block"); m.authorizations["signout-block"].sha256 = "0".repeat(64); } });
  const f = fleet(manifest);
  const code = await main(["--manifest", path, "--out", out], { run: f.run, log: quiet });
  assert.equal(code, 4);
  assert.deepEqual(f.calls.filter(MUTATING), []);
  const md = readFileSync(join(out, readdirSync(out).find((n) => n.endsWith(".md"))), "utf8");
  assert.match(md, /missing: authorization receipt .* digest/);
  assert.match(md, /missing: operation backend_readback/);
});

test("sign-out phase passes on a gate that blocks signed-out traffic and fails when the gate is disabled", async () => {
  const run = async (gate) => {
    const { manifest, path, out } = fixture({ operations: SIGNOUT_OPS, authorizations: receipt("signout-block") });
    const f = fleet(manifest, gate ? { gate } : {});
    const code = await main(["--manifest", path, "--out", out, "--phase", "signout-block"], { run: f.run, log: quiet });
    const results = JSON.parse(readFileSync(join(out, readdirSync(out).find((n) => n.startsWith("results-") && n.endsWith(".json"))), "utf8"));
    return { code, phase: results.phases[0], f };
  };
  const green = await run(null);
  assert.deepEqual(green.phase.checks.filter((c) => c.status !== "PASS").map((c) => c.id), []);
  assert.equal(green.code, 4, "a single-phase repeat is never a full pass");
  assert.equal(green.f.byPort[7701].uid, "uid-primary", "cleanup restored the primary account");

  const red = await run(() => true);
  const failed = red.phase.checks.filter((c) => c.status === "FAIL").map((c) => c.id);
  assert.equal(red.code, 1);
  assert.ok(failed.includes("signout-blocks-traffic"), failed.join());
  assert.ok(failed.includes("foreign-account-stays-blocked"), failed.join());
  assert.ok(failed.includes("signout-blocks-stream"), failed.join());
});

test("pair oracles catch re-pairing, access drift, new records and lost peers", () => {
  const rec = (over = {}) => ({ paired: { P: { public_key_sha256: "k", paired_at: 1, access_level: "full", online: true, ...over } }, incoming: 0 });
  assert.equal(o.pairContinuity("x", rec(), rec(), "P").status, "PASS");
  assert.match(o.pairContinuity("x", rec(), rec({ public_key_sha256: "k2" }), "P").reasons.join(), /re-pair/);
  assert.match(o.pairContinuity("x", rec(), rec({ paired_at: 2 }), "P").reasons.join(), /paired_at/);
  assert.match(o.pairContinuity("x", rec(), rec({ access_level: "read" }), "P").reasons.join(), /access_level/);
  assert.match(o.pairContinuity("x", rec(), rec({ online: false }), "P").reasons.join(), /offline/);
  assert.match(o.pairContinuity("x", rec(), { paired: { ...rec().paired, Q: {} }, incoming: 0 }, "P").reasons.join(), /new pair records/);
  assert.match(o.recordsPreserved("x", rec(), { paired: {} }, "P").reasons.join(), /record lost/);
  assert.equal(o.recordsPreserved("x", rec(), rec({ online: false }), "P").status, "PASS");
});

test("traffic oracles never read a hang as a block and require digest equality", () => {
  const good = { direction: "A->B", put_ok: true, get_ok: true, sent_sha256: "a", received_sha256: "a", timedOut: false };
  assert.equal(o.roundTrip("r", good).status, "PASS");
  assert.equal(o.roundTrip("r", { ...good, received_sha256: "b" }).status, "FAIL");
  assert.equal(o.trafficBlocked("b", [{ ...good, put_ok: false, put_error: "mesh_signed_out" }]).status, "PASS");
  assert.match(o.trafficBlocked("b", [{ ...good, put_ok: false, timedOut: true }]).reasons.join(), /not a proven block/);
  assert.match(o.trafficBlocked("b", [good]).reasons.join(), /still delivered/);
  assert.match(o.trafficBlocked("b", [{ ...good, put_ok: false }]).reasons.join(), /typed refusal/);
  const unpaired = "not_found: mesh request failed: HTTP 404: peer_not_paired: x is not a paired peer";
  assert.match(o.trafficBlocked("b", [{ ...good, put_ok: false, put_error: unpaired }]).reasons.join(), /removed instead of blocked/);
  assert.equal(o.ledgerDelivered("l", [{ seq: 1, id: "p1", sent_sha256: "a" }], [{ id: "p1", get_ok: true, received_sha256: "a" }]).status, "PASS");
  assert.match(o.ledgerDelivered("l", [{ seq: 1, id: "p1", sent_sha256: "a" }], [{ id: "p1", get_ok: false, get_error: "not_found" }]).reasons.join(), /not retrievable/);
  assert.equal(o.ledgerDelivered("l", [], []).status, "FAIL");
});

test("server limit, slot, event and modal oracles demand the exact Free refusal", () => {
  const refusal = { ok: false, error_code: "free_limit", limit_key: o.MESH_MACHINES_LIMIT_KEY, limit_value: 2 };
  assert.equal(o.limitRefusal("l", refusal).status, "PASS");
  assert.match(o.limitRefusal("l", { ...refusal, ok: true }).reasons.join(), /admitted a third/);
  assert.match(o.limitRefusal("l", { ...refusal, limit_value: 3 }).reasons.join(), /expected 2/);
  assert.match(o.limitRefusal("l", { ...refusal, limit_key: "agents" }).reasons.join(), /mesh.machines/);
  const m = { slots_used: 2, slots_limit: 2, installation_ids: ["a", "b"] };
  assert.equal(o.slotsHeld("s", m, { ...m, installation_ids: ["b", "a"] }, 2).status, "PASS");
  assert.match(o.slotsHeld("s", m, { ...m, slots_used: 3, installation_ids: ["a", "b", "c"] }, 2).reasons.join(), /slots_used 3[\s\S]*changed/);
  assert.equal(o.exactlyOnce("e", ["op1", "op2"], "op1").status, "PASS");
  assert.match(o.exactlyOnce("e", ["op1", "op1"], "op1").reasons.join(), /2 refusal events/);
  assert.equal(o.modalOpenedOnce("u", { open_count: 1 }).status, "PASS");
  assert.equal(o.modalOpenedOnce("u", { open_count: 2 }).status, "FAIL");
  assert.equal(o.machineRemoved("r", { installation_ids: ["a"], slots_used: 1 }, "b", { present: false }).status, "PASS");
  assert.match(o.machineRemoved("r", { installation_ids: ["a", "b"], slots_used: 2 }, "b", { present: true }).reasons.join(), /still listed[\s\S]*still published/);
});

test("expiry, backend-free, isolation and broadcast oracles bind exact observations", () => {
  const passProbe = { put_ok: true, get_ok: true, sent_sha256: "a", received_sha256: "a" };
  const blocked = { put_ok: false, put_error: "membership_expired", direction: "A->B" };
  const membership = { expires_at_ms: 5000 };
  const clock = { before: { now_ms: 4999 }, at: { now_ms: 5000 } };
  assert.equal(o.expiryBoundary("x", membership, passProbe, blocked, clock).status, "PASS");
  assert.match(o.expiryBoundary("x", membership, passProbe, passProbe, clock).reasons.join(), /still flowed/);
  assert.match(o.expiryBoundary("x", membership, blocked, blocked, clock).reasons.join(), /before the original expiry/);
  assert.match(o.expiryBoundary("x", membership, passProbe, blocked, { before: { now_ms: 4000 }, at: { now_ms: 5001 } }).reasons.join(), /expires_at - 1[\s\S]*exactly/);
  assert.equal(o.noPerMessageBackend("f", { connections: 4 }, { connections: 4 }, 10).status, "PASS");
  assert.match(o.noPerMessageBackend("f", { connections: 4 }, { connections: 14 }, 10).reasons.join(), /10 backend connection/);
  assert.match(o.noPerMessageBackend("f", {}, {}, 10).reasons.join(), /counters missing/);
  assert.equal(o.disjointAccounts("i", { installation_ids: ["a"] }, { installation_ids: ["z"] }).status, "PASS");
  assert.equal(o.disjointAccounts("i", { installation_ids: ["a"] }, { installation_ids: ["a"] }).status, "FAIL");
  const tree = { subcommands: [{ name: "mesh", subcommands: [{ name: "peers" }] }] };
  assert.equal(o.noBroadcast("n", tree, { discovered: 0 }, { broadcast_packets: 0 }).status, "PASS");
  assert.match(o.noBroadcast("n", { subcommands: [{ name: "mesh", subcommands: [{ name: "broadcast" }] }] }, { discovered: 1 }, { broadcast_packets: 3 }).reasons.join(), /broadcast\/discover[\s\S]*discovered[\s\S]*packets/);
  assert.equal(o.uiCopyShown("u", `x ${o.SIGNED_OUT_COPY}`).status, "PASS");
  assert.equal(o.uiCopyShown("u", "Signed out").status, "FAIL");
});

test("classification never promotes held, failed or partial evidence", () => {
  const pass = { status: "PASS" };
  assert.equal(classify({ refused: ["x"] }), CLASSIFICATION.REFUSED);
  assert.equal(classify({ preflight: [{ status: "FAIL" }] }), CLASSIFICATION.REFUSED);
  assert.equal(classify({ preflight: [pass], preflightOnly: true }), CLASSIFICATION.PREFLIGHT_PASS);
  assert.equal(classify({ preflight: [pass], phases: [] }), CLASSIFICATION.HELD);
  assert.equal(classify({ preflight: [pass], phases: [{ status: "RUN", checks: [pass] }, { status: "HELD", checks: [] }] }), CLASSIFICATION.HELD);
  assert.equal(classify({ preflight: [pass], phases: [{ status: "RUN", checks: [pass, { status: "FAIL" }] }] }), CLASSIFICATION.FAIL);
  assert.equal(classify({ preflight: [pass], phases: [{ status: "RUN", checks: [pass] }] }), CLASSIFICATION.PASS);
});

test("evidence redaction hashes identities and keys and drops addresses and secrets", () => {
  const r = redact({ identity: { uid: "u1", email: "e@x" }, paired: [{ public_key: "pk", addresses: ["1.2.3.4"], id_token: "t" }], tailscale_ip: "100.1.1.1" });
  assert.equal(r.identity.uid_sha256, sha("u1"));
  assert.ok(!JSON.stringify(r).includes("e@x") && !JSON.stringify(r).includes("1.2.3.4") && !JSON.stringify(r).includes("\"pk\""));
  assert.equal(r.paired[0].id_token, "[redacted]");
  assert.equal(r.tailscale_ip, "[address redacted]");
});

test("JSON extraction skips hint lines and command lookup reads the real generated CLI tree", () => {
  assert.deepEqual(extractJson("hint: {not json}\n{\"a\":[1,{\"b\":\"}\"}]}\ntrailer"), { a: [1, { b: "}" }] });
  assert.equal(extractJson("no json"), undefined);
  const tree = JSON.parse(readFileSync(join(repo, "generated", "cli", "cli-manifest.json"), "utf8"));
  assert.equal(cliCommandExists(tree, ["mesh", "peers"]), true);
  assert.equal(cliCommandExists(tree, ["paste", "get", "{peer_instance_id}"]), true);
  assert.equal(cliCommandExists(tree, ["mesh", "enroll-machine"]), false);
  assert.equal(cliCommandExists(tree, ["nonexistent"]), false);
});

test("a hung child is killed at its deadline and reported as timed out", async () => {
  const r = await runBounded([process.execPath, "-e", "setInterval(() => {}, 1000)"], { deadlineMs: 500 });
  assert.equal(r.timedOut, true);
  const done = await runBounded([process.execPath, "-e", "process.stdout.write(JSON.stringify({ok:1}))"], { deadlineMs: 10_000 });
  assert.deepEqual({ timedOut: done.timedOut, exit: done.exit, json: done.json }, { timedOut: false, exit: 0, json: { ok: 1 } });
});
