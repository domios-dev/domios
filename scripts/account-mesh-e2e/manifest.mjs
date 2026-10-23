import { createHash } from "node:crypto";
import { isAbsolute } from "node:path";

export const MANIFEST_SCHEMA = "domios.account-mesh-e2e.manifest/1";
export const PROD_API_PORT = 7685;
const DEFAULT_DEV_PORT = 7686;
export const LIMITS = Object.freeze({ commandMs: 60_000, phaseMs: 900_000, runMs: 3_600_000 });

export const ROLES = Object.freeze(["paired_a", "paired_b", "add_a", "add_b", "third"]);
const PLATFORMS = new Set(["windows", "macos", "linux"]);
const SHA40 = /^[0-9a-f]{40}$/;
const SHA256 = /^[0-9a-f]{64}$/;
const SLUG = /^[a-z0-9][a-z0-9-]{0,62}$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const VERSION = /^\d+\.\d+\.\d+$/;

// Arguments a manifest-supplied operation may never carry: each would unpair, re-pair, broadcast,
// replace a binary or change a host clock instead of letting the harness observe the product.
const FORBIDDEN_ARGV = [
  ["mesh", "disconnect"], ["mesh", "connect"], ["mesh", "approve"], ["mesh", "reject"],
  ["mesh", "unrevoke"], ["mesh", "broadcast"], ["update", "install"], ["app", "quit"], ["config", "set"],
];
export const PLACEHOLDERS = new Set(["target", "machine", "instance_id", "peer_instance_id", "peer_installation_id", "account_ref", "now_ms", "client_sha", "run_id"]);
const FORBIDDEN_TOKENS = new Set(["--mesh", "w32tm", "timedatectl", "Set-Date", "settimeofday", "hwclock"]);

export const HOOK_OPERATIONS = Object.freeze({
  upgrade: { mutates: true, fields: [] },
  restart: { mutates: true, fields: [] },
  login: { mutates: true, fields: [] },
  registry_block: { mutates: true, fields: [] },
  registry_unblock: { mutates: true, fields: [] },
  firebase_block: { mutates: true, fields: [] },
  firebase_unblock: { mutates: true, fields: [] },
  firebase_packets: { mutates: false, fields: ["connections"] },
  discovery_packets: { mutates: false, fields: ["broadcast_packets"] },
  time_control: { mutates: true, fields: ["now_ms"], isolated: true },
  firestore_probe: { mutates: false, fields: ["denied"] },
  backend_readback: { mutates: false, fields: ["sha", "project"] },
  ui_snapshot: { mutates: false, fields: ["text"] },
  ui_plans_modal: { mutates: false, fields: ["open_count"] },
});

export const PRODUCT_OPERATIONS = Object.freeze({
  enroll: { mutates: true, fields: ["ok", "error_code", "limit_key", "limit_value", "operation_id"] },
  membership: { mutates: false, fields: ["state", "expires_at_ms", "installation_id"] },
  machines: { mutates: false, fields: ["installation_ids", "slots_used", "slots_limit"] },
  add: { mutates: true, fields: ["ok", "error_code"] },
  revoke: { mutates: true, fields: ["ok"] },
  endpoint: { mutates: false, fields: ["present"] },
  signout_cleanup: { mutates: false, fields: ["cleanup"] },
  refusal_events: { mutates: false, fields: ["operation_ids"] },
  stream_probe: { mutates: false, fields: ["delivered"] },
});

const OPERATION_CONTRACTS = { ...HOOK_OPERATIONS, ...PRODUCT_OPERATIONS };

export function digest(value) {
  return createHash("sha256").update(typeof value === "string" ? value : JSON.stringify(value)).digest("hex");
}

function exactKeys(errors, where, value, required, optional = []) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    errors.push(`${where}: expected an object`);
    return false;
  }
  for (const key of required) if (!(key in value)) errors.push(`${where}.${key}: required`);
  const allowed = new Set([...required, ...optional]);
  for (const key of Object.keys(value)) if (!allowed.has(key)) errors.push(`${where}.${key}: unknown field`);
  return true;
}

function check(errors, ok, message) {
  if (!ok) errors.push(message);
}

function validateTransport(errors, where, transport) {
  if (!exactKeys(errors, where, transport, ["kind"], ["argv"])) return;
  if (transport.kind === "local") {
    check(errors, transport.argv === undefined, `${where}.argv: local transport takes no argv`);
  } else if (transport.kind === "prefix") {
    check(errors, Array.isArray(transport.argv) && transport.argv.length > 0 && transport.argv.every((a) => typeof a === "string" && a.length > 0),
      `${where}.argv: prefix transport needs a nonempty string argv`);
  } else {
    errors.push(`${where}.kind: must be local or prefix`);
  }
}

function validateTarget(errors, id, target, forbiddenPorts) {
  const where = `targets.${id}`;
  if (!exactKeys(errors, where, target, ["machine", "platform", "transport", "cli", "api_port", "instance_id", "account", "start"], ["instance", "web_ui_url"])) return;
  check(errors, target.start === "old" || target.start === "new", `${where}.start: old|new (the release the target must run before effects)`);
  check(errors, typeof target.machine === "string" && SLUG.test(target.machine), `${where}.machine: slug required`);
  check(errors, PLATFORMS.has(target.platform), `${where}.platform: windows|macos|linux`);
  validateTransport(errors, `${where}.transport`, target.transport);
  check(errors, typeof target.cli === "string" && (isAbsolute(target.cli) || target.cli.startsWith("/")), `${where}.cli: absolute path required`);
  check(errors, Number.isInteger(target.api_port) && target.api_port > 1024 && target.api_port < 65536, `${where}.api_port: integer port required`);
  check(errors, !forbiddenPorts.has(target.api_port), `${where}.api_port: ${target.api_port} is a forbidden (owner/prod) port`);
  check(errors, typeof target.instance_id === "string" && UUID.test(target.instance_id), `${where}.instance_id: lowercase uuid required`);
  if (target.instance !== undefined) check(errors, typeof target.instance === "string" && SLUG.test(target.instance), `${where}.instance: slug required`);
  // The CLI itself refuses a non-default port without a named instance rather than guess a socket.
  if (target.transport?.kind === "local" && target.api_port !== DEFAULT_DEV_PORT) {
    check(errors, target.instance !== undefined, `${where}.instance: local port ${target.api_port} needs the named instance that owns it`);
  }
  if (target.web_ui_url !== undefined) {
    let url = null;
    try { url = new URL(target.web_ui_url); } catch { /* reported below */ }
    check(errors, url && (url.protocol === "http:" || url.protocol === "https:") && Number(url.port) === target.api_port && url.pathname === "/" && !url.search && !url.username,
      `${where}.web_ui_url: must be http(s)://host:${target.api_port}/ with no path, query or credentials`);
  }
  check(errors, typeof target.account === "string", `${where}.account: account key required`);
}

function validateOperation(errors, key, op) {
  const where = `operations.${key}`;
  const contract = OPERATION_CONTRACTS[key];
  if (!contract) {
    errors.push(`${where}: unknown operation; known: ${Object.keys(OPERATION_CONTRACTS).join(", ")}`);
    return;
  }
  if (!exactKeys(errors, where, op, ["argv", "receipt", "fields"], ["isolated_clock"])) return;
  check(errors, Array.isArray(op.argv) && op.argv.length > 0 && op.argv.every((a) => typeof a === "string"), `${where}.argv: nonempty string array`);
  check(errors, typeof op.receipt === "string" && op.receipt.length >= 8, `${where}.receipt: name the accepted server/client receipt this argv comes from`);
  if (Array.isArray(op.argv)) {
    for (const token of op.argv) if (FORBIDDEN_TOKENS.has(token)) errors.push(`${where}.argv: forbidden token ${token}`);
    for (const token of op.argv) {
      for (const [, name] of String(token).matchAll(/\{([^}]*)\}/g)) {
        if (!PLACEHOLDERS.has(name)) errors.push(`${where}.argv: unknown placeholder {${name}}`);
      }
    }
    if (key in HOOK_OPERATIONS) check(errors, isAbsolute(op.argv[0] ?? "") || String(op.argv[0]).startsWith("/"), `${where}.argv[0]: hooks run an absolute Turner-owned command`);
    else check(errors, !isAbsolute(op.argv[0] ?? "") && !String(op.argv[0]).startsWith("-"), `${where}.argv[0]: product operations are target CLI subcommands`);
    for (const pair of FORBIDDEN_ARGV) {
      for (let i = 0; i + 1 < op.argv.length; i += 1) {
        if (op.argv[i] === pair[0] && op.argv[i + 1] === pair[1]) errors.push(`${where}.argv: forbidden command ${pair.join(" ")}`);
      }
    }
  }
  if (exactKeys(errors, `${where}.fields`, op.fields, contract.fields)) {
    for (const [field, path] of Object.entries(op.fields)) {
      check(errors, typeof path === "string" && /^[A-Za-z0-9_]+(\.[A-Za-z0-9_]+)*$/.test(path), `${where}.fields.${field}: dotted JSON path required`);
    }
  }
  if (contract.isolated) check(errors, op.isolated_clock === true, `${where}.isolated_clock: must be true; host clock changes are refused`);
  else check(errors, op.isolated_clock === undefined, `${where}.isolated_clock: only time_control takes it`);
}

export function validateManifest(manifest, { outDir } = {}) {
  const errors = [];
  if (!exactKeys(errors, "manifest", manifest, ["schema", "run_id", "evidence_dir", "release", "forbidden", "accounts", "targets", "roles", "operations", "authorizations", "deadlines"])) {
    return { ok: false, errors };
  }
  check(errors, manifest.schema === MANIFEST_SCHEMA, `schema: expected ${MANIFEST_SCHEMA}`);
  check(errors, typeof manifest.run_id === "string" && SLUG.test(manifest.run_id), "run_id: slug required");
  check(errors, typeof manifest.evidence_dir === "string" && isAbsolute(manifest.evidence_dir), "evidence_dir: absolute path required");
  if (outDir !== undefined) check(errors, manifest.evidence_dir === outDir, `evidence_dir: manifest names ${manifest.evidence_dir} but --out is ${outDir}`);

  const release = manifest.release;
  if (exactKeys(errors, "release", release, ["client_sha", "client_version", "old_client_sha", "old_client_version", "backend_sha", "backend_project"])) {
    for (const key of ["client_sha", "old_client_sha", "backend_sha"]) check(errors, SHA40.test(release[key] ?? ""), `release.${key}: full 40-hex sha required`);
    for (const key of ["client_version", "old_client_version"]) check(errors, VERSION.test(release[key] ?? ""), `release.${key}: x.y.z required`);
    check(errors, release.client_sha !== release.old_client_sha, "release: old and new client sha must differ for an upgrade oracle");
    check(errors, typeof release.backend_project === "string" && SLUG.test(release.backend_project), "release.backend_project: slug required");
  }

  const forbiddenPorts = new Set([PROD_API_PORT]);
  const forbiddenIds = new Set();
  if (exactKeys(errors, "forbidden", manifest.forbidden, ["api_ports", "instance_ids"])) {
    for (const port of manifest.forbidden.api_ports ?? []) forbiddenPorts.add(port);
    for (const id of manifest.forbidden.instance_ids ?? []) forbiddenIds.add(id);
    check(errors, manifest.forbidden.instance_ids?.length > 0, "forbidden.instance_ids: list the owner's live instance ids so they are refused");
  }

  const accounts = manifest.accounts;
  // Three Free accounts: a foreign sign-in on paired_a durably takes a foreign slot, so the Add
  // fixture cannot share that account without hitting its own two-machine limit.
  if (exactKeys(errors, "accounts", accounts, ["primary", "foreign", "add"])) {
    for (const key of ["primary", "foreign", "add"]) {
      if (exactKeys(errors, `accounts.${key}`, accounts[key], ["ref", "uid_sha256", "plan"])) {
        check(errors, SLUG.test(accounts[key].ref ?? ""), `accounts.${key}.ref: slug required (secrets stay in local stores)`);
        check(errors, SHA256.test(accounts[key].uid_sha256 ?? ""), `accounts.${key}.uid_sha256: sha256 of the account uid required`);
        check(errors, accounts[key].plan === "free", `accounts.${key}.plan: dedicated fixture accounts must be free`);
      }
    }
    const hashes = ["primary", "foreign", "add"].map((k) => accounts[k]?.uid_sha256);
    check(errors, new Set(hashes).size === 3, "accounts: primary, foreign and add must be three different accounts");
  }

  const targetIds = Object.keys(manifest.targets ?? {});
  check(errors, targetIds.length >= 3, "targets: at least three installations are required");
  const seenInstance = new Map();
  const seenEndpoint = new Map();
  for (const id of targetIds) {
    check(errors, /^[A-Z][A-Z0-9]{0,7}$/.test(id), `targets.${id}: id must be an uppercase label`);
    const target = manifest.targets[id];
    validateTarget(errors, id, target, forbiddenPorts);
    if (!target || typeof target !== "object") continue;
    if (forbiddenIds.has(target.instance_id)) errors.push(`targets.${id}.instance_id: owner instance ${target.instance_id} is forbidden`);
    if (seenInstance.has(target.instance_id)) errors.push(`targets.${id}.instance_id: duplicates targets.${seenInstance.get(target.instance_id)}`);
    seenInstance.set(target.instance_id, id);
    const endpoint = `${target.machine}:${target.api_port}:${target.instance ?? ""}`;
    if (seenEndpoint.has(endpoint)) errors.push(`targets.${id}: same machine/port/instance as targets.${seenEndpoint.get(endpoint)}`);
    seenEndpoint.set(endpoint, id);
    if (accounts && typeof target.account === "string" && !(target.account in accounts)) errors.push(`targets.${id}.account: unknown account ${target.account}`);
  }

  const roles = manifest.roles;
  if (exactKeys(errors, "roles", roles, ROLES)) {
    for (const role of ROLES) check(errors, targetIds.includes(roles[role]), `roles.${role}: must name a target`);
    const bound = ROLES.map((r) => roles[r]);
    check(errors, new Set(bound).size === bound.length, "roles: every role needs its own installation");
    const t = (role) => manifest.targets?.[roles[role]];
    if (t("paired_a") && t("paired_b") && t("third")) {
      check(errors, t("paired_a").machine !== t("paired_b").machine, "roles: paired_a and paired_b must be different real machines");
      check(errors, t("third").machine !== t("paired_a").machine && t("third").machine !== t("paired_b").machine, "roles: third must be a third machine");
      for (const role of ["paired_a", "paired_b", "third"]) check(errors, t(role).account === "primary", `roles.${role}: must use the primary account`);
      for (const role of ["paired_a", "paired_b"]) check(errors, t(role).start === "old", `roles.${role}: must start on the shipped old release`);
      check(errors, t("third").start === "new", "roles.third: must start on the reviewed new release");
    }
    if (t("add_a") && t("add_b")) {
      check(errors, t("add_a").machine !== t("add_b").machine, "roles: add_a and add_b must be different real machines");
      for (const role of ["add_a", "add_b"]) check(errors, t(role).account === "add", `roles.${role}: the Add fixture uses the add account so primary keeps exactly two slots`);
    }
  }

  const operations = manifest.operations ?? {};
  if (exactKeys(errors, "operations", operations, [], Object.keys(OPERATION_CONTRACTS))) {
    for (const [key, op] of Object.entries(operations)) validateOperation(errors, key, op);
  }

  if (manifest.authorizations && typeof manifest.authorizations === "object") {
    for (const [phase, auth] of Object.entries(manifest.authorizations)) {
      if (exactKeys(errors, `authorizations.${phase}`, auth, ["receipt", "sha256", "granted_by"])) {
        check(errors, isAbsolute(auth.receipt ?? ""), `authorizations.${phase}.receipt: absolute path required`);
        check(errors, SHA256.test(auth.sha256 ?? ""), `authorizations.${phase}.sha256: sha256 of the receipt file required`);
        check(errors, typeof auth.granted_by === "string" && auth.granted_by.length > 0, `authorizations.${phase}.granted_by: required`);
      }
    }
  } else {
    errors.push("authorizations: object required (may be empty for --preflight-only)");
  }

  if (exactKeys(errors, "deadlines", manifest.deadlines, ["command_ms", "phase_ms", "run_ms"])) {
    const d = manifest.deadlines;
    check(errors, Number.isInteger(d.command_ms) && d.command_ms > 0 && d.command_ms <= LIMITS.commandMs, `deadlines.command_ms: 1..${LIMITS.commandMs}`);
    check(errors, Number.isInteger(d.phase_ms) && d.phase_ms >= d.command_ms && d.phase_ms <= LIMITS.phaseMs, `deadlines.phase_ms: command_ms..${LIMITS.phaseMs}`);
    check(errors, Number.isInteger(d.run_ms) && d.run_ms >= d.phase_ms && d.run_ms <= LIMITS.runMs, `deadlines.run_ms: phase_ms..${LIMITS.runMs}`);
  }

  return { ok: errors.length === 0, errors, digest: errors.length === 0 ? digest(manifest) : null };
}

export function resolvePath(value, path) {
  let cursor = value;
  for (const segment of path.split(".")) {
    if (cursor === null || typeof cursor !== "object" || !(segment in cursor)) return undefined;
    cursor = cursor[segment];
  }
  return cursor;
}
