// Bitwarden secret-backend plugin (capability: secret_backend).
//
// Ports the native `secrets/bitwarden/` Rust impl to TS-on-QuickJS. Talks to the
// `bw` CLI via host.exec (always --nointeraction --response; secrets via env/stdin,
// never argv). Session/master-password persist in the per-plugin secret bucket
// (host.secret*). Returns {ok}/{error:{kind,...}} envelopes the JsSecretBackend
// adapter maps onto the core SecretStore trait; secretStatus returns a bare
// StoreStatus. Behaviour mirrors the native reference exactly (parity-gated).

var DEFAULT_SERVER = "https://vault.bitwarden.com";
var K_SESSION = "session";
var K_MASTER = "master_password";
var K_EMAIL = "login_email";
var K_CLIENT_ID = "api_client_id";
var K_CLIENT_SECRET = "api_client_secret";

// --- pure helpers (parity-tested against the native Rust) ---

var B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
function base64Encode(str) {
  // UTF-8 → base64. Pure JS (QuickJS has no btoa/Buffer).
  var bytes = [];
  for (var i = 0; i < str.length; i++) {
    var c = str.charCodeAt(i);
    if (c < 0x80) bytes.push(c);
    else if (c < 0x800) {
      bytes.push(0xc0 | (c >> 6), 0x80 | (c & 0x3f));
    } else if (c >= 0xd800 && c <= 0xdbff) {
      var c2 = str.charCodeAt(++i);
      var cp = 0x10000 + ((c & 0x3ff) << 10) + (c2 & 0x3ff);
      bytes.push(0xf0 | (cp >> 18), 0x80 | ((cp >> 12) & 0x3f), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
    } else {
      bytes.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
    }
  }
  var out = "";
  for (var j = 0; j < bytes.length; j += 3) {
    var b0 = bytes[j], b1 = bytes[j + 1], b2 = bytes[j + 2];
    out += B64[b0 >> 2];
    out += B64[((b0 & 3) << 4) | (b1 === undefined ? 0 : b1 >> 4)];
    out += b1 === undefined ? "=" : B64[((b1 & 15) << 2) | (b2 === undefined ? 0 : b2 >> 6)];
    out += b2 === undefined ? "=" : B64[b2 & 63];
  }
  return out;
}

// Translate a bw error message → a SecretError kind (mirror map_bw_message).
function mapBwError(msg) {
  var m = (msg || "").toLowerCase();
  if (m.indexOf("vault is locked") >= 0) return { kind: "locked" };
  if (m.indexOf("you are not logged in") >= 0) return { kind: "logged_out" };
  if (m.indexOf("not found") >= 0) return { kind: "not_found" };
  if (m.indexOf("more than one result") >= 0) return { kind: "ambiguous", candidates: [] };
  if (m.indexOf("already") >= 0 && m.indexOf("exists") >= 0) return { kind: "already_exists" };
  return { kind: "backend", message: msg && msg.length ? msg : "bw failed" };
}

// {object:"list",data:[...]} / {template:{...}} unwrap (mirror unwrap_data_envelope).
function unwrapData(v) {
  if (v && typeof v === "object" && v.data !== undefined) return v.data;
  if (v && typeof v === "object" && v.template !== undefined) return v.template;
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
    login: { username: null, password: req.value, totp: null, uris: [] },
  };
}

function isValidHttpUrl(s) {
  var t = (s || "").trim().toLowerCase();
  if (!(t.indexOf("http://") === 0 || t.indexOf("https://") === 0)) return false;
  var afterScheme = (s || "").split("://")[1] || "";
  var host = afterScheme.replace(/^\/+/, "").split(/[\/?#]/)[0];
  return host.length > 0;
}

// --- bw exec layer ---

function bw(args, opts) {
  opts = opts || {};
  var full = args.concat(["--nointeraction", "--response"]);
  var env = opts.env || {};
  if (opts.session) env.BW_SESSION = opts.session;
  var raw = host.exec(JSON.stringify({ bin: "bw", args: full, env: env, stdin: opts.stdin || null }));
  var ex;
  try { ex = JSON.parse(raw); } catch (e) { return { success: false, message: "exec parse: " + e }; }
  if (ex.error) return { success: false, message: ex.error };
  var resp;
  try { resp = JSON.parse((ex.stdout || "").trim()); } catch (e) {
    var combined = (ex.stderr && ex.stderr.length) ? ex.stderr : (ex.stdout || ("bw exited " + ex.code));
    return { success: false, message: combined };
  }
  return resp; // { success, data, message, errorCode }
}

// Run a session-scoped bw command, JIT-unlocking from the stored master pw if locked.
function runWithSession(args, stdin) {
  var session = host.secretGet(K_SESSION);
  if (!session) {
    tryUnlockFromStore();
    session = host.secretGet(K_SESSION);
    if (!session) return { error: { kind: "locked" } };
  }
  var r = bw(args, { session: session, stdin: stdin });
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
  var s = settings();
  if (s.serverUrl && s.serverUrl.length) return s.serverUrl;
  var u = host.secretGet("server_url");
  return u && u.length ? u : DEFAULT_SERVER;
}

function bwStatus() {
  var session = host.secretGet(K_SESSION);
  var r = bw(["status"], { session: session || undefined });
  if (!r.success) return null;
  return unwrapData(r.data);
}

function tryUnlockFromStore() {
  var master = host.secretGet(K_MASTER);
  if (!master) return;
  secretUnlock({ masterPassword: master, email: host.secretGet(K_EMAIL) });
}

// --- SecretStore trait surface (called by JsSecretBackend) ---

function secretStatus() {
  var endpoint = serverUrl();
  var s = bwStatus();
  if (!s) return { status: "unavailable", reason: "bw not available" };
  if (s.status === "unlocked") {
    if (s.userEmail) host.secretSet(K_EMAIL, s.userEmail);
    return { status: "unlocked", user: s.userEmail || null, transient: false, endpoint: s.serverUrl || endpoint };
  }
  if (s.status === "locked") return { status: "locked", endpoint: endpoint };
  return { status: "logged_out", endpoint: endpoint };
}

function secretUnlock(creds) {
  creds = creds || {};
  var hasPw = creds.masterPassword && creds.masterPassword.length;
  if (!hasPw && !creds.apiKeyClientId) {
    return { error: { kind: "bad_request", message: "master password (or API-key creds) required" } };
  }
  var server = serverUrl();
  if (!isValidHttpUrl(server)) {
    return { error: { kind: "bad_request", message: "invalid bitwarden server URL: " + server } };
  }

  var st = bwStatus();
  var loggedIn = st && st.status !== "unauthenticated";
  var currentServer = (st && st.serverUrl) || "";
  if (loggedIn && currentServer && currentServer !== server) {
    bw(["logout"]);
    host.secretDelete(K_SESSION);
    loggedIn = false;
  }
  if (currentServer !== server) {
    var cfg = bw(["config", "server", server]);
    if (!cfg.success) return { error: { kind: "backend", message: "could not point bw at " + server } };
  }

  st = bwStatus();
  loggedIn = st && st.status !== "unauthenticated";
  if (!loggedIn) {
    var login;
    if (creds.apiKeyClientId && creds.apiKeyClientSecret) {
      login = bw(["login", "--apikey"], { env: { BW_CLIENTID: creds.apiKeyClientId, BW_CLIENTSECRET: creds.apiKeyClientSecret } });
      if (login.success) {
        host.secretSet(K_CLIENT_ID, creds.apiKeyClientId);
        host.secretSet(K_CLIENT_SECRET, creds.apiKeyClientSecret);
      }
    } else if (creds.email) {
      var args = ["login", creds.email, "--passwordenv", "BW_PASSWORD"];
      if (creds.twoFactorToken) args = args.concat(["--method", "0", "--code", creds.twoFactorToken]);
      login = bw(args, { env: { BW_PASSWORD: creds.masterPassword } });
    } else {
      return { error: { kind: "bad_request", message: "either email + master_password or api_key_* required" } };
    }
    if (!login.success && (login.message || "").toLowerCase().indexOf("already logged in") < 0) {
      return { error: mapBwError(login.message) };
    }
  }

  var unlocked = bw(["unlock", "--passwordenv", "BW_PASSWORD", "--raw"], { env: { BW_PASSWORD: creds.masterPassword } });
  if (!unlocked.success) return { error: mapBwError(unlocked.message) };
  var d = unlocked.data;
  var token = typeof d === "string" ? d : (d && d.raw) || (d && d.template && d.template.raw);
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
  var t = Date.parse(s);
  return isNaN(t) ? null : t;
}

function toRef(c) {
  return {
    item_id: c.id,
    name: c.name,
    folder: c.folderId || null,
    organization: c.organizationId || null,
    collection: (c.collectionIds && c.collectionIds[0]) || null,
    last_modified: isoToMs(c.revisionDate),
  };
}

function findByName(name) {
  var r = runWithSession(["list", "items", "--search", name]);
  if (r.error) return r;
  var exact = (r.ok || []).filter(function (c) { return c.type === 1 && c.name.toLowerCase() === name.toLowerCase(); });
  if (exact.length === 0) return { error: { kind: "not_found" } };
  if (exact.length > 1) return { error: { kind: "ambiguous", candidates: exact.map(function (c) { return c.id; }) } };
  return { ok: exact[0] };
}

function resolveFolderId(name) {
  if (!name) return { ok: null };
  var r = runWithSession(["list", "folders"]);
  if (r.error) return r;
  var f = (r.ok || []).find(function (x) { return x.name && x.name.toLowerCase() === name.toLowerCase(); });
  return { ok: f ? f.id : null };
}

function resolveOrCreateFolder(name) {
  var existing = resolveFolderId(name);
  if (existing.error) return existing;
  if (existing.ok) return existing;
  var encoded = base64Encode(JSON.stringify({ name: name }));
  var r = runWithSession(["create", "folder", encoded]);
  if (r.error) return r;
  return { ok: (r.ok && r.ok.id) || null };
}

function secretSave(req) {
  if (!req.name) return { error: { kind: "bad_request", message: "name required" } };
  if (!req.value) return { error: { kind: "bad_request", message: "value must not be empty" } };
  var existing = findByName(req.name);
  var existingItem = existing.ok || null;
  if (existingItem && !req.overwrite) return { error: { kind: "already_exists" } };

  var folderId = null;
  if (req.folder) {
    var f = resolveOrCreateFolder(req.folder);
    if (f.error) return f;
    folderId = f.ok;
  }
  var cipher = buildLoginCipher(req, existingItem ? existingItem.id : null, folderId);
  var encoded = base64Encode(JSON.stringify(cipher));
  var r = existingItem
    ? runWithSession(["edit", "item", existingItem.id, encoded])
    : runWithSession(["create", "item", encoded]);
  if (r.error) return r;
  runWithSession(["sync"]);
  return { ok: toRef(r.ok) };
}

function secretGetItem(nameOrId) {
  var r = runWithSession(["get", "item", nameOrId]);
  if (r.error) return r;
  var item = r.ok;
  var value = item.login && item.login.password;
  if (value === undefined || value === null) return { error: { kind: "backend", message: "item has no login.password field" } };
  return { ok: { item_id: item.id, name: item.name, value: value, notes: item.notes || null } };
}

function secretList(filter) {
  filter = filter || {};
  var args = ["list", "items"];
  if (filter.organization) args = args.concat(["--organizationid", filter.organization]);
  if (filter.collection) args = args.concat(["--collectionid", filter.collection]);
  var r = runWithSession(args);
  if (r.error) return r;
  var items = (r.ok || []).filter(function (c) { return c.type === 1; });
  if (filter.folder) {
    var fid = resolveFolderId(filter.folder);
    if (fid.error) return fid;
    items = items.filter(function (c) { return (c.folderId || null) === (fid.ok || null); });
  }
  items.sort(function (a, b) { return a.name.toLowerCase() < b.name.toLowerCase() ? -1 : 1; });
  if (filter.limit) items = items.slice(0, filter.limit);
  return { ok: items.map(toRef) };
}

function secretSearch(query) {
  var q = (query || "").trim();
  if (!q) return { error: { kind: "bad_request", message: "query must not be empty" } };
  var r = runWithSession(["list", "items", "--search", q]);
  if (r.error) return r;
  return { ok: (r.ok || []).filter(function (c) { return c.type === 1; }).map(toRef) };
}

function secretDelete(nameOrId) {
  var item = runWithSession(["get", "item", nameOrId]);
  if (item.error) return item;
  var r = runWithSession(["delete", "item", item.ok.id, "--permanent"]);
  if (r.error) return r;
  runWithSession(["sync"]);
  return { ok: true };
}

function secretOrganizations() {
  var r = runWithSession(["list", "organizations"]);
  if (r.error) return r;
  return { ok: (r.ok || []).map(function (v) { return { id: v.id, name: v.name }; }) };
}

function secretCollections(orgId) {
  var args = ["list", "collections"];
  if (orgId) args = args.concat(["--organizationid", orgId]);
  var r = runWithSession(args);
  if (r.error) return r;
  return { ok: (r.ok || []).map(function (v) { return { id: v.id, name: v.name, organization_id: v.organizationId || null }; }) };
}

function secretSync() {
  var r = runWithSession(["sync"]);
  if (r.error) return r;
  return { ok: true };
}

function secretInit() {
  var s = bwStatus();
  if (!s || s.status === "unlocked") return { ok: true };
  tryUnlockFromStore();
  return { ok: true };
}

module.exports.default = {
  secretStatus: secretStatus,
  secretUnlock: secretUnlock,
  secretLock: secretLock,
  secretLogout: secretLogout,
  secretSave: secretSave,
  secretGetItem: secretGetItem,
  secretList: secretList,
  secretSearch: secretSearch,
  secretDelete: secretDelete,
  secretOrganizations: secretOrganizations,
  secretCollections: secretCollections,
  secretSync: secretSync,
  secretInit: secretInit,
  // exported for parity tests (pure logic, no host.exec)
  __test_base64: base64Encode,
  __test_mapError: mapBwError,
  __test_unwrap: unwrapData,
  __test_buildCipher: buildLoginCipher,
};
