"use strict";
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// plugins/bitwarden/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  default: () => plugin_default
});
module.exports = __toCommonJS(plugin_exports);
var DEFAULT_SERVER = "https://vault.bitwarden.com";
var K_SESSION = "session";
var K_MASTER = "master_password";
var K_EMAIL = "login_email";
var K_CLIENT_ID = "api_client_id";
var K_CLIENT_SECRET = "api_client_secret";
var B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
function base64Encode(str) {
  const bytes = [];
  for (let i = 0; i < str.length; i++) {
    const c = str.charCodeAt(i);
    if (c < 128) bytes.push(c);
    else if (c < 2048) {
      bytes.push(192 | c >> 6, 128 | c & 63);
    } else if (c >= 55296 && c <= 56319) {
      const c2 = str.charCodeAt(++i);
      const cp = 65536 + ((c & 1023) << 10) + (c2 & 1023);
      bytes.push(240 | cp >> 18, 128 | cp >> 12 & 63, 128 | cp >> 6 & 63, 128 | cp & 63);
    } else {
      bytes.push(224 | c >> 12, 128 | c >> 6 & 63, 128 | c & 63);
    }
  }
  let out = "";
  for (let j = 0; j < bytes.length; j += 3) {
    const b0 = bytes[j], b1 = bytes[j + 1], b2 = bytes[j + 2];
    out += B64[b0 >> 2];
    out += B64[(b0 & 3) << 4 | (b1 === void 0 ? 0 : b1 >> 4)];
    out += b1 === void 0 ? "=" : B64[(b1 & 15) << 2 | (b2 === void 0 ? 0 : b2 >> 6)];
    out += b2 === void 0 ? "=" : B64[b2 & 63];
  }
  return out;
}
function mapBwError(msg) {
  const m = (msg || "").toLowerCase();
  if (m.indexOf("vault is locked") >= 0) return { kind: "locked" };
  if (m.indexOf("you are not logged in") >= 0) return { kind: "logged_out" };
  if (m.indexOf("not found") >= 0) return { kind: "not_found" };
  if (m.indexOf("more than one result") >= 0) return { kind: "ambiguous", candidates: [] };
  if (m.indexOf("already") >= 0 && m.indexOf("exists") >= 0) return { kind: "already_exists" };
  return { kind: "backend", message: msg && msg.length ? msg : "bw failed" };
}
function unwrapData(v) {
  if (v && typeof v === "object" && v.data !== void 0) {
    return v.data;
  }
  if (v && typeof v === "object" && v.template !== void 0) {
    return v.template;
  }
  return v;
}
function buildLoginCipher(req, existingId, folderId) {
  return {
    id: existingId || null,
    type: 1,
    name: req.name,
    notes: req.notes || null,
    folderId: folderId || null,
    organizationId: req.organization || null,
    collectionIds: req.collection ? [req.collection] : [],
    favorite: false,
    login: { username: null, password: req.value, totp: null, uris: [] }
  };
}
function isValidHttpUrl(s) {
  const t = (s || "").trim().toLowerCase();
  if (!(t.indexOf("http://") === 0 || t.indexOf("https://") === 0)) return false;
  const afterScheme = (s || "").split("://")[1] || "";
  const h = afterScheme.replace(/^\/+/, "").split(/[\/?#]/)[0];
  return h.length > 0;
}
function bw(args, opts) {
  opts = opts || {};
  const full = args.concat(["--nointeraction", "--response"]);
  const env = opts.env || {};
  if (opts.session) env.BW_SESSION = opts.session;
  const raw = host.exec(JSON.stringify({ bin: "bw", args: full, env, stdin: opts.stdin || null }));
  let ex;
  try {
    ex = JSON.parse(raw);
  } catch (e) {
    return { success: false, message: "exec parse: " + e };
  }
  if (ex.error) return { success: false, message: ex.error };
  let resp;
  try {
    resp = JSON.parse((ex.stdout || "").trim());
  } catch (e) {
    const combined = ex.stderr && ex.stderr.length ? ex.stderr : ex.stdout || "bw exited " + ex.code;
    return { success: false, message: combined };
  }
  return resp;
}
function runWithSession(args, stdin) {
  let session = host.secretGet(K_SESSION);
  if (!session) {
    tryUnlockFromStore();
    session = host.secretGet(K_SESSION);
    if (!session) return { error: { kind: "locked" } };
  }
  const r = bw(args, { session, stdin });
  if (!r.success) return { error: mapBwError(r.message) };
  return { ok: unwrapData(r.data) };
}
function settings() {
  try {
    return JSON.parse(host.settingsJson()) || {};
  } catch (e) {
    return {};
  }
}
function serverUrl() {
  const s = settings();
  if (s.serverUrl && s.serverUrl.length) return s.serverUrl;
  const u = host.secretGet("server_url");
  return u && u.length ? u : DEFAULT_SERVER;
}
function bwStatus() {
  const session = host.secretGet(K_SESSION);
  const r = bw(["status"], { session: session || void 0 });
  if (!r.success) return null;
  return unwrapData(r.data);
}
function tryUnlockFromStore() {
  const master = host.secretGet(K_MASTER);
  if (!master) return;
  secretUnlock({ masterPassword: master, email: host.secretGet(K_EMAIL) || void 0 });
}
function secretStatus() {
  const endpoint = serverUrl();
  const s = bwStatus();
  if (!s) return { status: "unavailable", reason: "bw not available" };
  if (s.status === "unlocked") {
    if (s.userEmail) host.secretSet(K_EMAIL, s.userEmail);
    return { status: "unlocked", user: s.userEmail || null, transient: false, endpoint: s.serverUrl || endpoint };
  }
  if (s.status === "locked") return { status: "locked", endpoint };
  return { status: "logged_out", endpoint };
}
function secretUnlock(creds) {
  creds = creds || {};
  const hasPw = creds.masterPassword && creds.masterPassword.length;
  if (!hasPw && !creds.apiKeyClientId) {
    return { error: { kind: "bad_request", message: "master password (or API-key creds) required" } };
  }
  const server = serverUrl();
  if (!isValidHttpUrl(server)) {
    return { error: { kind: "bad_request", message: "invalid bitwarden server URL: " + server } };
  }
  let st = bwStatus();
  let loggedIn = !!st && st.status !== "unauthenticated";
  const currentServer = st && st.serverUrl || "";
  if (loggedIn && currentServer && currentServer !== server) {
    bw(["logout"]);
    host.secretDelete(K_SESSION);
    loggedIn = false;
  }
  if (currentServer !== server) {
    const cfg = bw(["config", "server", server]);
    if (!cfg.success) return { error: { kind: "backend", message: "could not point bw at " + server } };
  }
  st = bwStatus();
  loggedIn = !!st && st.status !== "unauthenticated";
  if (!loggedIn) {
    let login;
    if (creds.apiKeyClientId && creds.apiKeyClientSecret) {
      login = bw(["login", "--apikey"], { env: { BW_CLIENTID: creds.apiKeyClientId, BW_CLIENTSECRET: creds.apiKeyClientSecret } });
      if (login.success) {
        host.secretSet(K_CLIENT_ID, creds.apiKeyClientId);
        host.secretSet(K_CLIENT_SECRET, creds.apiKeyClientSecret);
      }
    } else if (creds.email) {
      let args = ["login", creds.email, "--passwordenv", "BW_PASSWORD"];
      if (creds.twoFactorToken) args = args.concat(["--method", "0", "--code", creds.twoFactorToken]);
      login = bw(args, { env: { BW_PASSWORD: creds.masterPassword } });
    } else {
      return { error: { kind: "bad_request", message: "either email + master_password or api_key_* required" } };
    }
    if (!login.success && (login.message || "").toLowerCase().indexOf("already logged in") < 0) {
      return { error: mapBwError(login.message) };
    }
  }
  const unlocked = bw(["unlock", "--passwordenv", "BW_PASSWORD", "--raw"], { env: { BW_PASSWORD: creds.masterPassword } });
  if (!unlocked.success) return { error: mapBwError(unlocked.message) };
  const d = unlocked.data;
  let token = typeof d === "string" ? d : d && d.raw || d && d.template && d.template.raw;
  token = (token || "").trim();
  if (!token) return { error: { kind: "backend", message: "bw unlock returned empty session" } };
  host.secretSet(K_SESSION, token);
  if (!creds.apiKeyClientId) {
    host.secretSet(K_MASTER, creds.masterPassword);
    if (creds.email) host.secretSet(K_EMAIL, creds.email);
  }
  return { ok: true };
}
function secretLock() {
  bw(["lock"]);
  host.secretDelete(K_SESSION);
  return { ok: true };
}
function secretLogout() {
  bw(["logout"]);
  host.secretDelete(K_SESSION);
  host.secretDelete(K_MASTER);
  host.secretDelete(K_EMAIL);
  host.secretDelete(K_CLIENT_ID);
  host.secretDelete(K_CLIENT_SECRET);
  return { ok: true };
}
function isoToMs(s) {
  if (!s) return null;
  const t = Date.parse(s);
  return isNaN(t) ? null : t;
}
function toRef(c) {
  return {
    item_id: c.id,
    name: c.name,
    folder: c.folderId || null,
    organization: c.organizationId || null,
    collection: c.collectionIds && c.collectionIds[0] || null,
    last_modified: isoToMs(c.revisionDate)
  };
}
function findByName(name) {
  const r = runWithSession(["list", "items", "--search", name]);
  if ("error" in r) return r;
  const exact = (r.ok || []).filter((c) => c.type === 1 && c.name.toLowerCase() === name.toLowerCase());
  if (exact.length === 0) return { error: { kind: "not_found" } };
  if (exact.length > 1) return { error: { kind: "ambiguous", candidates: exact.map((c) => c.id) } };
  return { ok: exact[0] };
}
function resolveFolderId(name) {
  if (!name) return { ok: null };
  const r = runWithSession(["list", "folders"]);
  if ("error" in r) return r;
  const f = (r.ok || []).find((x) => x.name && x.name.toLowerCase() === name.toLowerCase());
  return { ok: f ? f.id : null };
}
function resolveOrCreateFolder(name) {
  const existing = resolveFolderId(name);
  if ("error" in existing) return existing;
  if (existing.ok) return existing;
  const encoded = base64Encode(JSON.stringify({ name }));
  const r = runWithSession(["create", "folder", encoded]);
  if ("error" in r) return r;
  const created = r.ok;
  return { ok: created && created.id || null };
}
function secretSave(req) {
  if (!req.name) return { error: { kind: "bad_request", message: "name required" } };
  if (!req.value) return { error: { kind: "bad_request", message: "value must not be empty" } };
  const existing = findByName(req.name);
  const existingItem = "ok" in existing && existing.ok || null;
  if (existingItem && !req.overwrite) return { error: { kind: "already_exists" } };
  let folderId = null;
  if (req.folder) {
    const f = resolveOrCreateFolder(req.folder);
    if ("error" in f) return f;
    folderId = f.ok;
  }
  const cipher = buildLoginCipher(req, existingItem ? existingItem.id : null, folderId);
  const encoded = base64Encode(JSON.stringify(cipher));
  const r = existingItem ? runWithSession(["edit", "item", existingItem.id, encoded]) : runWithSession(["create", "item", encoded]);
  if ("error" in r) return r;
  runWithSession(["sync"]);
  return { ok: toRef(r.ok) };
}
function secretGetItem(nameOrId) {
  const r = runWithSession(["get", "item", nameOrId]);
  if ("error" in r) return r;
  const item = r.ok;
  const value = item.login && item.login.password;
  if (value === void 0 || value === null) return { error: { kind: "backend", message: "item has no login.password field" } };
  return { ok: { item_id: item.id, name: item.name, value, notes: item.notes || null } };
}
function secretList(filter) {
  filter = filter || {};
  let args = ["list", "items"];
  if (filter.organization) args = args.concat(["--organizationid", filter.organization]);
  if (filter.collection) args = args.concat(["--collectionid", filter.collection]);
  const r = runWithSession(args);
  if ("error" in r) return r;
  let items = (r.ok || []).filter((c) => c.type === 1);
  if (filter.folder) {
    const fid = resolveFolderId(filter.folder);
    if ("error" in fid) return fid;
    items = items.filter((c) => (c.folderId || null) === (fid.ok || null));
  }
  items.sort((a, b) => a.name.toLowerCase() < b.name.toLowerCase() ? -1 : 1);
  if (filter.limit) items = items.slice(0, filter.limit);
  return { ok: items.map(toRef) };
}
function secretSearch(query) {
  const q = (query || "").trim();
  if (!q) return { error: { kind: "bad_request", message: "query must not be empty" } };
  const r = runWithSession(["list", "items", "--search", q]);
  if ("error" in r) return r;
  return { ok: (r.ok || []).filter((c) => c.type === 1).map(toRef) };
}
function secretDelete(nameOrId) {
  const item = runWithSession(["get", "item", nameOrId]);
  if ("error" in item) return item;
  const r = runWithSession(["delete", "item", item.ok.id, "--permanent"]);
  if ("error" in r) return r;
  runWithSession(["sync"]);
  return { ok: true };
}
function secretOrganizations() {
  const r = runWithSession(["list", "organizations"]);
  if ("error" in r) return r;
  return { ok: (r.ok || []).map((v) => ({ id: v.id, name: v.name })) };
}
function secretCollections(orgId) {
  let args = ["list", "collections"];
  if (orgId) args = args.concat(["--organizationid", orgId]);
  const r = runWithSession(args);
  if ("error" in r) return r;
  return { ok: (r.ok || []).map((v) => ({ id: v.id, name: v.name, organization_id: v.organizationId || null })) };
}
function secretSync() {
  const r = runWithSession(["sync"]);
  if ("error" in r) return r;
  return { ok: true };
}
function secretInit() {
  const s = bwStatus();
  if (!s || s.status === "unlocked") return { ok: true };
  tryUnlockFromStore();
  return { ok: true };
}
function renderGlance() {
  const s = secretStatus();
  const nodes = [];
  if (s.status === "unlocked") {
    nodes.push({ kind: "badge", label: "Unlocked", tone: "ok" });
    nodes.push({ kind: "keyVal", key: "Account", value: s.user || "\u2014" });
    if (s.endpoint) nodes.push({ kind: "keyVal", key: "Server", value: s.endpoint });
  } else if (s.status === "locked") {
    nodes.push({ kind: "badge", label: "Locked", tone: "warn" });
    nodes.push({ kind: "text", text: "Vault is locked \u2014 unlock it in settings.", style: { tone: "muted" } });
  } else if (s.status === "unavailable") {
    nodes.push({ kind: "badge", label: "bw not found", tone: "danger" });
    nodes.push({ kind: "note", body: s.reason || "The Bitwarden CLI isn't available.", level: "error" });
  } else {
    nodes.push({ kind: "badge", label: "Not signed in", tone: "danger" });
    if (s.endpoint) nodes.push({ kind: "keyVal", key: "Server", value: s.endpoint });
  }
  nodes.push({ kind: "divider" });
  nodes.push({ kind: "button", label: "Open settings", action: "settings" });
  return { title: "Bitwarden", nodes };
}
function viewCall(method, args) {
  args = args || {};
  if (method === "status") return secretStatus();
  if (method === "serverUrl") return { url: serverUrl() };
  if (method === "setServerUrl") {
    host.secretSet("server_url", args.url || "");
    return { ok: true };
  }
  if (method === "unlock") {
    return secretUnlock({
      masterPassword: args.masterPassword,
      email: args.email,
      twoFactorToken: args.twoFactorToken,
      apiKeyClientId: args.apiKeyClientId,
      apiKeyClientSecret: args.apiKeyClientSecret
    });
  }
  if (method === "signout") return secretLogout();
  if (method === "organizations") {
    const o = secretOrganizations();
    return { organizations: "ok" in o && o.ok || [] };
  }
  if (method === "collections") {
    const c = secretCollections(args.organization);
    return { collections: "ok" in c && c.ok || [] };
  }
  return { error: "unknown view method: " + method };
}
var plugin = {
  secretStatus,
  renderGlance,
  viewCall,
  secretUnlock,
  secretLock,
  secretLogout,
  secretSave,
  secretGetItem,
  secretList,
  secretSearch,
  secretDelete,
  secretOrganizations,
  secretCollections,
  secretSync,
  secretInit,
  // exported for parity tests (pure logic, no host.exec)
  __test_base64: base64Encode,
  __test_mapError: mapBwError,
  __test_unwrap: unwrapData,
  __test_buildCipher: buildLoginCipher
};
var plugin_default = plugin;
