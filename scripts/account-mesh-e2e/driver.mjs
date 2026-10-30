import { randomBytes } from "node:crypto";
import { HOOK_OPERATIONS, PRODUCT_OPERATIONS, ROLES, digest, resolvePath } from "./manifest.mjs";
import { commandEvidence } from "./journal.mjs";
import { buildMatches, projectAccount, projectPeers } from "./oracles.mjs";
import { cliArgv, cliCommandExists, runBounded, targetEnv } from "./runner.mjs";

export const BUILTIN_COMMANDS = Object.freeze([
  ["identify"], ["cli-manifest"], ["mesh", "status"], ["mesh", "peers"], ["account", "status"],
  ["account", "logout"], ["paste", "put"], ["paste", "get"], ["paste", "rm"],
]);
const PROBE_TTL = "30m";

export class DeadlineError extends Error {}

export function createDriver({ manifest, journal, run = runBounded, clock = Date.now, env = process.env }) {
  const runDeadline = clock() + manifest.deadlines.run_ms;
  let phaseDeadline = runDeadline;
  const targetOf = (role) => {
    const id = ROLES.includes(role) ? manifest.roles[role] : role;
    const target = manifest.targets[id];
    if (!target) throw new Error(`unknown role or target ${role}`);
    return { id, ...target };
  };
  const budget = () => {
    const remaining = Math.min(phaseDeadline, runDeadline) - clock();
    if (remaining <= 0) throw new DeadlineError("phase or run deadline exhausted");
    return Math.min(manifest.deadlines.command_ms, remaining);
  };
  const record = (label, result, extra = {}) => {
    const file = journal.artifact(label, { ...extra, ...commandEvidence(result) });
    journal.event("command", { label, file, exit: result.exit, timed_out: result.timedOut });
    return file;
  };

  async function cli(role, args, { input, label } = {}) {
    const target = targetOf(role);
    const full = args.includes("--json") ? args : [...args, "--json"];
    const result = await run(cliArgv(target, full), { env: targetEnv(target, env), input, deadlineMs: budget() });
    record(label ?? `${target.id}-${args.slice(0, 2).join("-")}`, result, { target: target.id });
    return { ...result, json: result.exit === 0 ? result.json?.result : result.json };
  }

  function vars(role, extra = {}) {
    const target = targetOf(role);
    return { target: target.id, machine: target.machine, instance_id: target.instance_id, client_sha: manifest.release.client_sha, run_id: manifest.run_id, ...extra };
  }

  function fill(argv, values) {
    return argv.map((token) => token.replace(/\{([a-z_]+)\}/g, (_, name) => {
      if (values[name] === undefined || values[name] === null) throw new Error(`placeholder {${name}} has no value`);
      return String(values[name]);
    }));
  }

  async function op(key, role, extra = {}) {
    const spec = manifest.operations[key];
    if (!spec) throw new Error(`operation ${key} not supplied`);
    const values = vars(role, extra);
    let result;
    if (key in HOOK_OPERATIONS) {
      result = await run(fill(spec.argv, values), { env, deadlineMs: budget() });
    } else {
      const target = targetOf(role);
      const args = fill(spec.argv, values);
      result = await run(cliArgv(target, args.includes("--json") ? args : [...args, "--json"]), { env: targetEnv(target, env), deadlineMs: budget() });
      if (result.exit === 0) result = { ...result, json: result.json?.result };
    }
    const fields = {};
    for (const [field, path] of Object.entries(spec.fields)) fields[field] = resolvePath(result.json, path);
    record(`${key}-${values.target}`, result, { operation: key, receipt: spec.receipt, fields });
    return { ...fields, exit: result.exit, timedOut: result.timedOut, ok_exit: result.exit === 0 };
  }

  async function peers(role) {
    const result = await cli(role, ["mesh", "peers"]);
    return projectPeers(result.json);
  }

  async function account(role) {
    const result = await cli(role, ["account", "status"]);
    return projectAccount(result.json);
  }

  async function meshStatus(role) {
    return (await cli(role, ["mesh", "status"])).json ?? null;
  }

  async function cliManifest(role) {
    return (await cli(role, ["cli-manifest"])).json ?? null;
  }

  const errorOf = (result) => (result.json?.error?.code ? `${result.json.error.code}: ${String(result.json.error.message ?? "").slice(0, 160)}` : result.timedOut ? "timeout" : result.exit === 0 ? null : `exit ${result.exit}`);

  // Put on the peer through the real mesh, then read it back by id: both legs must be answered by the peer.
  async function probe(fromRole, toRole, seq = 0) {
    const peer = targetOf(toRole);
    const body = `amesh:${manifest.run_id}:${targetOf(fromRole).id}->${peer.id}:${seq}:${randomBytes(8).toString("hex")}`;
    const put = await cli(fromRole, ["paste", "put", "--from-stdin", "--ttl", PROBE_TTL, "--mesh", peer.instance_id], { input: body, label: `probe-put-${seq}` });
    const id = put.exit === 0 ? put.json?.id ?? null : null;
    const out = { direction: `${targetOf(fromRole).id}->${peer.id}`, seq, id, sent_sha256: digest(body), put_ok: Boolean(id), put_error: id ? null : errorOf(put), timedOut: put.timedOut };
    if (!id) return { ...out, get_ok: false, get_error: null, received_sha256: null };
    const fetched = await fetch(fromRole, toRole, id);
    return { ...out, ...fetched, timedOut: out.timedOut || fetched.timedOut };
  }

  async function fetch(fromRole, toRole, id) {
    const get = await cli(fromRole, ["paste", "get", id, "--mesh", targetOf(toRole).instance_id], { label: "probe-get" });
    const ok = get.exit === 0 && typeof get.json?.body === "string";
    return { id, get_ok: ok, get_error: ok ? null : errorOf(get), received_sha256: ok ? digest(get.json.body) : null, timedOut: get.timedOut };
  }

  async function cleanupPaste(fromRole, toRole, id) {
    const rm = await cli(fromRole, ["paste", "rm", id, "--mesh", targetOf(toRole).instance_id], { label: "probe-rm" });
    return rm.exit === 0;
  }

  return {
    manifest, journal, targetOf, cli, op, peers, account, meshStatus, cliManifest, probe, fetch, cleanupPaste,
    setPhaseDeadline(ms) { phaseDeadline = Math.min(runDeadline, clock() + ms); },
    clearPhaseDeadline() { phaseDeadline = runDeadline; },
  };
}

const check = (id, reasons, evidence = {}) => ({ id, status: reasons.length ? "FAIL" : "PASS", reasons, evidence });

export async function preflight(driver) {
  const { manifest } = driver;
  const checks = [];
  const peersByTarget = {};
  for (const id of Object.keys(manifest.targets)) {
    const target = driver.targetOf(id);
    const reasons = [];
    const identified = await driver.cli(id, ["identify"]);
    const connection = identified.json?.connection;
    if (!connection) {
      const why = identified.json?.error?.message ?? (identified.timedOut ? "timed out" : `exit ${identified.exit}`);
      checks.push(check(`target-${id}`, [`unreachable: ${String(why).slice(0, 240)}`], { machine: target.machine }));
      continue;
    } else {
      if (connection.port !== target.api_port) reasons.push(`CLI resolved port ${connection.port}, manifest ${target.api_port}`);
      if ((connection.instance ?? null) !== (target.instance ?? null)) reasons.push(`CLI resolved instance ${connection.instance}, manifest ${target.instance ?? null}`);
    }
    const status = await driver.meshStatus(id);
    if (!status) reasons.push("mesh status --json unanswered");
    else {
      if (status.instance_id !== target.instance_id) reasons.push(`daemon instance ${status.instance_id} is not ${target.instance_id}`);
      if (status.api_port !== target.api_port) reasons.push(`daemon api_port ${status.api_port} is not ${target.api_port}`);
      if (manifest.forbidden.instance_ids.includes(status.instance_id)) reasons.push("daemon is a forbidden owner instance");
      const sha = target.start === "old" ? manifest.release.old_client_sha : manifest.release.client_sha;
      if (!buildMatches(status, sha)) reasons.push(`daemon build ${status.build} does not match ${target.start} release ${sha}`);
    }
    const tree = await driver.cliManifest(id);
    if (!tree) reasons.push("cli-manifest unanswered");
    else {
      for (const path of BUILTIN_COMMANDS) if (!cliCommandExists(tree, path)) reasons.push(`target CLI lacks ${path.join(" ")}`);
      if (target.start === "new") {
        for (const [key, spec] of Object.entries(manifest.operations)) {
          if (key in PRODUCT_OPERATIONS && !cliCommandExists(tree, spec.argv)) reasons.push(`target CLI lacks operation ${key}: ${spec.argv.join(" ")}`);
        }
      }
    }
    const acct = await driver.account(id);
    if (!acct) reasons.push("account status --json unanswered");
    peersByTarget[id] = await driver.peers(id);
    if (!peersByTarget[id]) reasons.push("mesh peers --json unanswered");
    checks.push(check(`target-${id}`, reasons, { machine: target.machine, build: status?.build ?? null, signed_in: acct?.signed_in ?? null }));
  }
  const r = manifest.roles;
  const linked = (a, b) => Boolean(peersByTarget[r[a]]?.paired?.[driver.targetOf(b).instance_id]);
  const topology = [];
  if (!linked("paired_a", "paired_b") || !linked("paired_b", "paired_a")) topology.push("paired_a and paired_b must share an existing bilateral pair");
  if (linked("add_a", "add_b") || linked("add_b", "add_a")) topology.push("the Add fixture must start unlinked");
  for (const role of ["paired_a", "paired_b"]) if (linked("third", role) || linked(role, "third")) topology.push(`third must not be linked to ${role}`);
  checks.push(check("topology", topology));
  return checks;
}
