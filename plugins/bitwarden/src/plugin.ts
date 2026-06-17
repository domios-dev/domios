// Bitwarden secret-backend plugin (capability: secret_backend).
// Authored in TypeScript against @codeterm/plugin-sdk and compiled to a
// QuickJS-compatible plugin.js by scripts/build-plugin.mjs.
//
// Ports the native `secrets/bitwarden/` Rust impl to TS-on-QuickJS. Talks to the
// `bw` CLI via host.exec (always --nointeraction --response; secrets via env/stdin,
// never argv). Session/master-password persist in the per-plugin secret bucket
// (host.secret*). Returns {ok}/{error:{kind,...}} envelopes the JsSecretBackend
// adapter maps onto the core SecretStore trait; secretStatus returns a bare
// StoreStatus. Behaviour mirrors the native reference exactly (parity-gated).
import type {
  GlanceView,
  PluginModule,
  SecretCreds,
  SecretError,
  SecretListFilter,
  SecretSaveRequest,
  SecretStatus,
  ViewNode,
} from "@codeterm/plugin-sdk";

const DEFAULT_SERVER = "https://vault.bitwarden.com";
const K_SESSION = "session";
const K_MASTER = "master_password";
const K_EMAIL = "login_email";
const K_CLIENT_ID = "api_client_id";
const K_CLIENT_SECRET = "api_client_secret";

type Envelope<T = unknown> = { ok: T } | { error: SecretError };

// bw --response JSON envelope.
interface BwResponse {
  success: boolean;
  data?: unknown;
  message?: string;
  errorCode?: unknown;
}

// --- pure helpers (parity-tested against the native Rust) ---

const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
function base64Encode(str: string): string {
  // UTF-8 → base64. Pure JS (QuickJS has no btoa/Buffer).
  const bytes: number[] = [];
  for (let i = 0; i < str.length; i++) {
    const c = str.charCodeAt(i);
    if (c < 0x80) bytes.push(c);
    else if (c < 0x800) {
      bytes.push(0xc0 | (c >> 6), 0x80 | (c & 0x3f));
    } else if (c >= 0xd800 && c <= 0xdbff) {
      const c2 = str.charCodeAt(++i);
      const cp = 0x10000 + ((c & 0x3ff) << 10) + (c2 & 0x3ff);
      bytes.push(0xf0 | (cp >> 18), 0x80 | ((cp >> 12) & 0x3f), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
    } else {
      bytes.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
    }
  }
  let out = "";
  for (let j = 0; j < bytes.length; j += 3) {
    const b0 = bytes[j], b1 = bytes[j + 1], b2 = bytes[j + 2];
    out += B64[b0 >> 2];
    out += B64[((b0 & 3) << 4) | (b1 === undefined ? 0 : b1 >> 4)];
    out += b1 === undefined ? "=" : B64[((b1 & 15) << 2) | (b2 === undefined ? 0 : b2 >> 6)];
    out += b2 === undefined ? "=" : B64[b2 & 63];
  }
  return out;
}

// Translate a bw error message → a SecretError kind (mirror map_bw_message).
function mapBwError(msg: string | undefined): SecretError {
  const m = (msg || "").toLowerCase();
  if (m.indexOf("vault is locked") >= 0) return { kind: "locked" };
  if (m.indexOf("you are not logged in") >= 0) return { kind: "logged_out" };
  if (m.indexOf("not found") >= 0) return { kind: "not_found" };
  if (m.indexOf("more than one result") >= 0) return { kind: "ambiguous", candidates: [] };
  if (m.indexOf("already") >= 0 && m.indexOf("exists") >= 0) return { kind: "already_exists" };
  return { kind: "backend", message: msg && msg.length ? msg : "bw failed" };
}

// {object:"list",data:[...]} / {template:{...}} unwrap (mirror unwrap_data_envelope).
function unwrapData(v: unknown): unknown {
  if (v && typeof v === "object" && (v as Record<string, unknown>).data !== undefined) {
    return (v as Record<string, unknown>).data;
  }
  if (v && typeof v === "object" && (v as Record<string, unknown>).template !== undefined) {
    return (v as Record<string, unknown>).template;
  }
  return v;
}

interface CipherItem {
  id: string;
  type: number;
  name: string;
  notes?: string | null;
  folderId?: string | null;
  organizationId?: string | null;
  collectionIds?: string[];
  revisionDate?: string;
  login?: { password?: string | null } | null;
}

function buildLoginCipher(req: SecretSaveRequest, existingId: string | null, folderId: string | null): unknown {
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

function isValidHttpUrl(s: string): boolean {
  const t = (s || "").trim().toLowerCase();
  if (!(t.indexOf("http://") === 0 || t.indexOf("https://") === 0)) return false;
  const afterScheme = (s || "").split("://")[1] || "";
  const h = afterScheme.replace(/^\/+/, "").split(/[\/?#]/)[0];
  return h.length > 0;
}

// --- bw exec layer ---

interface BwOpts {
  env?: Record<string, string>;
  session?: string;
  stdin?: string | null;
}

function bw(args: string[], opts?: BwOpts): BwResponse {
  opts = opts || {};
  const full = args.concat(["--nointeraction", "--response"]);
  const env = opts.env || {};
  if (opts.session) env.BW_SESSION = opts.session;
  const raw = host.exec(JSON.stringify({ bin: "bw", args: full, env: env, stdin: opts.stdin || null }));
  let ex: { error?: string; stdout?: string; stderr?: string; code?: number };
  try { ex = JSON.parse(raw); } catch (e) { return { success: false, message: "exec parse: " + e }; }
  if (ex.error) return { success: false, message: ex.error };
  let resp: BwResponse;
  try { resp = JSON.parse((ex.stdout || "").trim()); } catch (e) {
    const combined = (ex.stderr && ex.stderr.length) ? ex.stderr : (ex.stdout || ("bw exited " + ex.code));
    return { success: false, message: combined };
  }
  return resp; // { success, data, message, errorCode }
}

// Run a session-scoped bw command, JIT-unlocking from the stored master pw if locked.
function runWithSession(args: string[], stdin?: string): Envelope {
  let session = host.secretGet(K_SESSION);
  if (!session) {
    tryUnlockFromStore();
    session = host.secretGet(K_SESSION);
    if (!session) return { error: { kind: "locked" } };
  }
  const r = bw(args, { session: session, stdin: stdin });
  if (!r.success) return { error: mapBwError(r.message) };
  return { ok: unwrapData(r.data) };
}

interface BitwardenSettings {
  serverUrl?: string;
}

function settings(): BitwardenSettings {
  try {
    return (JSON.parse(host.settingsJson()) as BitwardenSettings) || {};
  } catch (e) {
    return {};
  }
}

function serverUrl(): string {
  const s = settings();
  if (s.serverUrl && s.serverUrl.length) return s.serverUrl;
  const u = host.secretGet("server_url");
  return u && u.length ? u : DEFAULT_SERVER;
}

interface BwStatus {
  status: string;
  userEmail?: string;
  serverUrl?: string;
}

function bwStatus(): BwStatus | null {
  const session = host.secretGet(K_SESSION);
  const r = bw(["status"], { session: session || undefined });
  if (!r.success) return null;
  return unwrapData(r.data) as BwStatus;
}

function tryUnlockFromStore(): void {
  const master = host.secretGet(K_MASTER);
  if (!master) return;
  secretUnlock({ masterPassword: master, email: host.secretGet(K_EMAIL) || undefined });
}

// --- SecretStore trait surface (called by JsSecretBackend) ---

function secretStatus(): SecretStatus {
  const endpoint = serverUrl();
  const s = bwStatus();
  if (!s) return { status: "unavailable", reason: "bw not available" };
  if (s.status === "unlocked") {
    if (s.userEmail) host.secretSet(K_EMAIL, s.userEmail);
    return { status: "unlocked", user: s.userEmail || null, transient: false, endpoint: s.serverUrl || endpoint };
  }
  if (s.status === "locked") return { status: "locked", endpoint: endpoint };
  return { status: "logged_out", endpoint: endpoint };
}

function secretUnlock(creds: SecretCreds): Envelope<true> {
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
  const currentServer = (st && st.serverUrl) || "";
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
    let login: BwResponse;
    if (creds.apiKeyClientId && creds.apiKeyClientSecret) {
      login = bw(["login", "--apikey"], { env: { BW_CLIENTID: creds.apiKeyClientId as string, BW_CLIENTSECRET: creds.apiKeyClientSecret as string } });
      if (login.success) {
        host.secretSet(K_CLIENT_ID, creds.apiKeyClientId as string);
        host.secretSet(K_CLIENT_SECRET, creds.apiKeyClientSecret as string);
      }
    } else if (creds.email) {
      let args = ["login", creds.email, "--passwordenv", "BW_PASSWORD"];
      if (creds.twoFactorToken) args = args.concat(["--method", "0", "--code", creds.twoFactorToken as string]);
      login = bw(args, { env: { BW_PASSWORD: creds.masterPassword as string } });
    } else {
      return { error: { kind: "bad_request", message: "either email + master_password or api_key_* required" } };
    }
    if (!login.success && (login.message || "").toLowerCase().indexOf("already logged in") < 0) {
      return { error: mapBwError(login.message) };
    }
  }

  const unlocked = bw(["unlock", "--passwordenv", "BW_PASSWORD", "--raw"], { env: { BW_PASSWORD: creds.masterPassword as string } });
  if (!unlocked.success) return { error: mapBwError(unlocked.message) };
  const d = unlocked.data as string | { raw?: string; template?: { raw?: string } } | undefined;
  let token = typeof d === "string" ? d : (d && d.raw) || (d && d.template && d.template.raw);
  token = (token || "").trim();
  if (!token) return { error: { kind: "backend", message: "bw unlock returned empty session" } };

  host.secretSet(K_SESSION, token);
  if (!creds.apiKeyClientId) {
    host.secretSet(K_MASTER, creds.masterPassword as string);
    if (creds.email) host.secretSet(K_EMAIL, creds.email);
  }
  return { ok: true };
}

function secretLock(): Envelope<true> {
  bw(["lock"]);
  host.secretDelete(K_SESSION);
  return { ok: true };
}

function secretLogout(): Envelope<true> {
  bw(["logout"]);
  host.secretDelete(K_SESSION);
  host.secretDelete(K_MASTER);
  host.secretDelete(K_EMAIL);
  host.secretDelete(K_CLIENT_ID);
  host.secretDelete(K_CLIENT_SECRET);
  return { ok: true };
}

function isoToMs(s: string | undefined): number | null {
  if (!s) return null;
  const t = Date.parse(s);
  return isNaN(t) ? null : t;
}

function toRef(c: CipherItem): unknown {
  return {
    item_id: c.id,
    name: c.name,
    folder: c.folderId || null,
    organization: c.organizationId || null,
    collection: (c.collectionIds && c.collectionIds[0]) || null,
    last_modified: isoToMs(c.revisionDate),
  };
}

function findByName(name: string): Envelope<CipherItem> {
  const r = runWithSession(["list", "items", "--search", name]);
  if ("error" in r) return r;
  const exact = ((r.ok as CipherItem[]) || []).filter((c) => c.type === 1 && c.name.toLowerCase() === name.toLowerCase());
  if (exact.length === 0) return { error: { kind: "not_found" } };
  if (exact.length > 1) return { error: { kind: "ambiguous", candidates: exact.map((c) => c.id) } };
  return { ok: exact[0] };
}

interface FolderItem { id: string; name: string }

function resolveFolderId(name: string | null | undefined): Envelope<string | null> {
  if (!name) return { ok: null };
  const r = runWithSession(["list", "folders"]);
  if ("error" in r) return r;
  const f = ((r.ok as FolderItem[]) || []).find((x) => x.name && x.name.toLowerCase() === name.toLowerCase());
  return { ok: f ? f.id : null };
}

function resolveOrCreateFolder(name: string): Envelope<string | null> {
  const existing = resolveFolderId(name);
  if ("error" in existing) return existing;
  if (existing.ok) return existing;
  const encoded = base64Encode(JSON.stringify({ name: name }));
  const r = runWithSession(["create", "folder", encoded]);
  if ("error" in r) return r;
  const created = r.ok as FolderItem | null;
  return { ok: (created && created.id) || null };
}

function secretSave(req: SecretSaveRequest): Envelope {
  if (!req.name) return { error: { kind: "bad_request", message: "name required" } };
  if (!req.value) return { error: { kind: "bad_request", message: "value must not be empty" } };
  const existing = findByName(req.name);
  const existingItem = ("ok" in existing && existing.ok) || null;
  if (existingItem && !req.overwrite) return { error: { kind: "already_exists" } };

  let folderId: string | null = null;
  if (req.folder) {
    const f = resolveOrCreateFolder(req.folder);
    if ("error" in f) return f;
    folderId = f.ok;
  }
  const cipher = buildLoginCipher(req, existingItem ? existingItem.id : null, folderId);
  const encoded = base64Encode(JSON.stringify(cipher));
  const r = existingItem
    ? runWithSession(["edit", "item", existingItem.id, encoded])
    : runWithSession(["create", "item", encoded]);
  if ("error" in r) return r;
  runWithSession(["sync"]);
  return { ok: toRef(r.ok as CipherItem) };
}

function secretGetItem(nameOrId: string): Envelope {
  const r = runWithSession(["get", "item", nameOrId]);
  if ("error" in r) return r;
  const item = r.ok as CipherItem;
  const value = item.login && item.login.password;
  if (value === undefined || value === null) return { error: { kind: "backend", message: "item has no login.password field" } };
  return { ok: { item_id: item.id, name: item.name, value: value, notes: item.notes || null } };
}

function secretList(filter?: SecretListFilter): Envelope {
  filter = filter || {};
  let args = ["list", "items"];
  if (filter.organization) args = args.concat(["--organizationid", filter.organization]);
  if (filter.collection) args = args.concat(["--collectionid", filter.collection]);
  const r = runWithSession(args);
  if ("error" in r) return r;
  let items = ((r.ok as CipherItem[]) || []).filter((c) => c.type === 1);
  if (filter.folder) {
    const fid = resolveFolderId(filter.folder);
    if ("error" in fid) return fid;
    items = items.filter((c) => (c.folderId || null) === (fid.ok || null));
  }
  items.sort((a, b) => (a.name.toLowerCase() < b.name.toLowerCase() ? -1 : 1));
  if (filter.limit) items = items.slice(0, filter.limit);
  return { ok: items.map(toRef) };
}

function secretSearch(query: string): Envelope {
  const q = (query || "").trim();
  if (!q) return { error: { kind: "bad_request", message: "query must not be empty" } };
  const r = runWithSession(["list", "items", "--search", q]);
  if ("error" in r) return r;
  return { ok: ((r.ok as CipherItem[]) || []).filter((c) => c.type === 1).map(toRef) };
}

function secretDelete(nameOrId: string): Envelope<true> {
  const item = runWithSession(["get", "item", nameOrId]);
  if ("error" in item) return item;
  const r = runWithSession(["delete", "item", (item.ok as CipherItem).id, "--permanent"]);
  if ("error" in r) return r;
  runWithSession(["sync"]);
  return { ok: true };
}

interface OrgItem { id: string; name: string; organizationId?: string | null }

function secretOrganizations(): Envelope {
  const r = runWithSession(["list", "organizations"]);
  if ("error" in r) return r;
  return { ok: ((r.ok as OrgItem[]) || []).map((v) => ({ id: v.id, name: v.name })) };
}

function secretCollections(orgId?: string): Envelope {
  let args = ["list", "collections"];
  if (orgId) args = args.concat(["--organizationid", orgId]);
  const r = runWithSession(args);
  if ("error" in r) return r;
  return { ok: ((r.ok as OrgItem[]) || []).map((v) => ({ id: v.id, name: v.name, organization_id: v.organizationId || null })) };
}

function secretSync(): Envelope<true> {
  const r = runWithSession(["sync"]);
  if ("error" in r) return r;
  return { ok: true };
}

function secretInit(): Envelope<true> {
  const s = bwStatus();
  if (!s || s.status === "unlocked") return { ok: true };
  tryUnlockFromStore();
  return { ok: true };
}

// Glance: a quick peek at the vault connection — not the unlock form.
function renderGlance(): GlanceView {
  const s = secretStatus();
  const nodes: ViewNode[] = [];
  if (s.status === "unlocked") {
    nodes.push({ kind: "badge", label: "Unlocked", tone: "ok" });
    nodes.push({ kind: "keyVal", key: "Account", value: s.user || "—" });
    if (s.endpoint) nodes.push({ kind: "keyVal", key: "Server", value: s.endpoint });
  } else if (s.status === "locked") {
    nodes.push({ kind: "badge", label: "Locked", tone: "warn" });
    nodes.push({ kind: "text", text: "Vault is locked — unlock it in settings.", style: { tone: "muted" } });
  } else if (s.status === "unavailable") {
    nodes.push({ kind: "badge", label: "bw not found", tone: "danger" });
    nodes.push({ kind: "note", body: s.reason || "The Bitwarden CLI isn't available.", level: "error" });
  } else {
    nodes.push({ kind: "badge", label: "Not signed in", tone: "danger" });
    if (s.endpoint) nodes.push({ kind: "keyVal", key: "Server", value: s.endpoint });
  }
  nodes.push({ kind: "divider" });
  nodes.push({ kind: "button", label: "Open settings", action: "settings" });
  return { title: "Bitwarden", nodes: nodes };
}

interface ViewArgs {
  url?: string;
  masterPassword?: string;
  email?: string;
  twoFactorToken?: string;
  apiKeyClientId?: string;
  apiKeyClientSecret?: string;
  organization?: string;
}

// Bridge entry for the plugin's iframe UI (capability: view). The iframe owns the
// connection panel (status / unlock / server URL); each call lands here and is
// routed to the plugin's own secret methods — never to arbitrary exports.
function viewCall(method: string, args: ViewArgs): unknown {
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
      apiKeyClientSecret: args.apiKeyClientSecret,
    });
  }
  if (method === "signout") return secretLogout();
  if (method === "organizations") {
    const o = secretOrganizations();
    return { organizations: ("ok" in o && o.ok) || [] };
  }
  if (method === "collections") {
    const c = secretCollections(args.organization);
    return { collections: ("ok" in c && c.ok) || [] };
  }
  return { error: "unknown view method: " + method };
}

const plugin: PluginModule = {
  secretStatus,
  renderGlance,
  viewCall: viewCall as PluginModule["viewCall"],
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
  __test_buildCipher: buildLoginCipher,
};

export default plugin;
