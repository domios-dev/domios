import type {FetchOpts, FetchResult, RuntimeProfileLimitsSource} from "@codeterm/plugin-sdk";

export interface ProfileLimitsRequest {profile:string; source:RuntimeProfileLimitsSource}
export type FetchPort = <T>(opts:FetchOpts, then:(result:FetchResult) => T) => T;
export type CredentialPublicPort = <T>(id:string, then:(fields:Readonly<Record<string,string>> | null) => T) => T;
export interface LimitsPorts {fetch:FetchPort; credentialPublic:CredentialPublicPort}
export interface BalanceAmount {currency:string; amount:string}
/** Failures name their cause and never carry a response body. */
export interface ProfileLimitsFailure {error:string}
export type ProfileLimitsReply = string | ProfileLimitsFailure;

interface BalanceReader {
  url:string;
  parse(body:string): {available:boolean; amounts:BalanceAmount[]} | null;
}
interface QuotaLimit {label:string; used_percent:number; resets_at:number | null}
export interface ParsedQuotaReading {title:string; limits:QuotaLimit[]}

const decimal = /^-?\d{1,15}(?:\.\d{1,8})?$/;
const currency = /^[A-Z]{3}$/;
const MAX_QUOTA_BUCKETS = 128;
const MAX_QUOTA_RESET_SECONDS = 31 * 24 * 60 * 60;
const STRICT_ISO_8601 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:?\d{2})$/;
const PRINTABLE_LABEL = /^[^\u0000-\u001f\u007f]+$/;

function deepseekBalance(body:string): {available:boolean; amounts:BalanceAmount[]} | null {
  const value = JSON.parse(body) as {is_available?:unknown; balance_infos?:unknown};
  if (typeof value.is_available !== "boolean" || !Array.isArray(value.balance_infos) || value.balance_infos.length > 8) return null;
  const amounts = value.balance_infos.map((entry:{currency?:unknown; total_balance?:unknown}) =>
    typeof entry?.currency === "string" && currency.test(entry.currency) && typeof entry.total_balance === "string" && decimal.test(entry.total_balance)
      ? {currency:entry.currency, amount:entry.total_balance} : null);
  return amounts.every(Boolean) ? {available:value.is_available, amounts:amounts as BalanceAmount[]} : null;
}

/** Balance protocols this plugin implements, named by a profile's declared `reader`. */
const readers:Readonly<Record<string,BalanceReader>> = {
  "deepseek-balance": {url:"https://api.deepseek.com/user/balance", parse:deepseekBalance},
};

function snapshot(billingMode:"api" | "subscription", details:Record<string,unknown>, nowMs:number): string {
  return JSON.stringify({
    provider:"", account_id:null, captured_at_ms:nowMs, billing_mode:billingMode, limits_reported:true,
    session_pct:null, session_resets_at_ms:null, weekly_pct:null, weekly_resets_at_ms:null,
    spend_cents:null, spend_limit_cents:null, spend_remaining_cents:null, spend_resets_at_ms:null, raw_text:"",
    usage_details:{limits:[], dollar_credits:[], extra_usage:null, ...details},
  });
}

function readJson(fetch:FetchPort, opts:FetchOpts, subject:string, parse:(body:string) => ProfileLimitsReply): ProfileLimitsReply {
  return fetch<ProfileLimitsReply>(opts, result => {
    if (result.error) return {error:result.error};
    if (result.status !== 200) return {error:`${subject} endpoint answered HTTP ${result.status}`};
    if (!result.body || result.body.length > 64 * 1024) return {error:`${subject} response is empty or too large`};
    try {
      return parse(result.body);
    } catch {
      return {error:`${subject} response has an unexpected format`};
    }
  });
}

const CODE_ASSIST = "https://cloudcode-pa.googleapis.com/v1internal";
const GEMINI_QUOTA = "Gemini quota";
const modelId = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;

function codeAssistRequest(method:string, body:unknown): FetchOpts {
  return {
    url:`${CODE_ASSIST}:${method}`, method:"POST", timeoutMs:15000, headers:{"Content-Type":"application/json"}, body:JSON.stringify(body),
    credential:{id:"geminiOauth", headers:{Authorization:"Bearer {accessToken}"}},
  };
}

/** One row per model at its lowest remaining fraction; a bucket without a model or fraction is skipped, as Gemini CLI skips it. */
function codeAssistLimits(body:string): QuotaLimit[] | null {
  const buckets = (JSON.parse(body) as {buckets?:unknown}).buckets;
  if (!Array.isArray(buckets) || buckets.length > 128) return null;
  const lowest = new Map<string,{fraction:number; resetTime:unknown}>();
  for (const bucket of buckets as {modelId?:unknown; remainingFraction?:unknown; resetTime?:unknown}[]) {
    if (bucket?.modelId == null || bucket.remainingFraction == null) continue;
    const fraction = bucket.remainingFraction;
    if (typeof bucket.modelId !== "string" || !modelId.test(bucket.modelId) || typeof fraction !== "number" || !(fraction >= 0 && fraction <= 1)) return null;
    const known = lowest.get(bucket.modelId);
    if (!known || fraction < known.fraction) lowest.set(bucket.modelId, {fraction, resetTime:bucket.resetTime});
  }
  return [...lowest].map(([label, {fraction, resetTime}]) => {
    const resets = typeof resetTime === "string" ? Date.parse(resetTime) : NaN;
    return {label, used_percent:Math.round((1 - fraction) * 1000) / 10, resets_at:Number.isFinite(resets) ? resets : null};
  }).sort((a, b) => b.used_percent - a.used_percent || a.label.localeCompare(b.label));
}

function noGoogleQuotaReason(mode:string | undefined): string {
  if (mode === "gemini-api-key") return "Gemini API-key quota has no public endpoint; check usage in Google AI Studio";
  if (mode === "vertex-ai") return "Vertex AI quota is not readable here; check usage in the Google Cloud console";
  if (mode === undefined) return "Gemini has no readable sign-in yet, so no quota can be read";
  return "This Gemini sign-in mode reports no quota";
}

/** Only a tab signed in with Google spends this quota, so a leftover login never answers for another sign-in mode. */
function geminiCodeAssistQuota({fetch, credentialPublic}:LimitsPorts, nowMs:number): ProfileLimitsReply {
  return credentialPublic<ProfileLimitsReply>("geminiSettings", settings => {
    if (settings?.selectedType !== "oauth-personal") return {error:noGoogleQuotaReason(settings?.selectedType)};
    return credentialPublic<ProfileLimitsReply>("geminiOauth", login => {
      if (!login) return {error:"Gemini's Google sign-in is not in a file Domios can read, so no quota can be read"};
      if (!(Number(login.expiresAt) > nowMs)) return {error:"Gemini's sign-in token has expired; quota reads again once Gemini renews it"};
      const metadata = {ideType:"IDE_UNSPECIFIED", platform:"PLATFORM_UNSPECIFIED", pluginType:"GEMINI"};
      return readJson(fetch, codeAssistRequest("loadCodeAssist", {metadata}), GEMINI_QUOTA, body => {
        const project = (JSON.parse(body) as {cloudaicompanionProject?:unknown}).cloudaicompanionProject;
        if (typeof project !== "string" || !project) return {error:"This Google account uses its own GOOGLE_CLOUD_PROJECT, which Domios cannot read, so no quota can be read"};
        return readJson(fetch, codeAssistRequest("retrieveUserQuota", {project}), GEMINI_QUOTA, quotaBody => {
          const limits = codeAssistLimits(quotaBody);
          if (!limits) return {error:`${GEMINI_QUOTA} response has an unexpected format`};
          if (!limits.length) return {error:"Gemini reported no model quota for this account"};
          return snapshot("subscription", {quota:{title:"Gemini model quota", limits}}, nowMs);
        });
      });
    });
  });
}

function isPlainObject(value:unknown): value is Record<string,unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function validBucketLabel(label:string): string | null {
  const trimmed = label.trim();
  if (!trimmed || trimmed.length > 128 || !PRINTABLE_LABEL.test(trimmed)) return null;
  return trimmed;
}

function parseBucketResetAt(resetTime:unknown, resetInSeconds:unknown, nowMs:number): {ok:true; value:number | null} | {ok:false} {
  if (resetTime !== undefined && resetTime !== null) {
    if (typeof resetTime !== "string") return {ok:false};
    const trimmed = resetTime.trim();
    if (trimmed) {
      if (trimmed.length > 128 || !STRICT_ISO_8601.test(trimmed)) return {ok:false};
      const parsed = Date.parse(trimmed);
      if (!Number.isFinite(parsed)) return {ok:false};
      return {ok:true, value:Math.trunc(parsed)};
    }
  }
  if (resetInSeconds !== undefined && resetInSeconds !== null) {
    if (typeof resetInSeconds !== "number" || !Number.isFinite(resetInSeconds) || resetInSeconds < 0 || resetInSeconds > MAX_QUOTA_RESET_SECONDS) {
      return {ok:false};
    }
    if (resetInSeconds === 0) return {ok:true, value:null};
    return {ok:true, value:Math.trunc(nowMs + Math.round(resetInSeconds * 1000))};
  }
  return {ok:true, value:null};
}

const LOWER_WINDOWS = new Set(["hourly", "daily", "weekly", "monthly", "session"]);
const DURATION_SEGMENT = /^\d+[smhdw]$/i;
const SHORT_DURATION = /^\d+[smh]$/i;
const DAY_DURATION = /^\d+d$/i;
const WEEK_DURATION = /^\d+w$/i;
const DIGIT_PREFIX_CODE = /^(\d+)([a-z]+)$/i;

function formatBucketLabel(rawKey:string): string {
  const parts = rawKey.trim().split(/[-_]+/).filter(Boolean);
  if (!parts.length) return rawKey.trim();
  return parts.map((part, index) => {
    const lower = part.toLowerCase();
    if (DURATION_SEGMENT.test(lower)) return lower;
    if (index > 0 && LOWER_WINDOWS.has(lower)) return lower;
    const digitCode = DIGIT_PREFIX_CODE.exec(part);
    if (digitCode) return `${digitCode[1]}${digitCode[2].toUpperCase()}`;
    return lower.charAt(0).toUpperCase() + lower.slice(1);
  }).join(" ");
}

function bucketSortMeta(rawKey:string): {digitGroup:number; group:string; windowRank:number} {
  const parts = rawKey.trim().split(/[-_]+/).filter(Boolean);
  const group = (parts[0] ?? rawKey.trim()).toLowerCase();
  const digitGroup = /^\d/.test(group) ? 1 : 0;
  const last = parts.length > 1 ? parts[parts.length - 1].toLowerCase() : "";
  const windowRank = last === "session" || last === "hourly" || SHORT_DURATION.test(last) ? 0
    : last === "daily" || DAY_DURATION.test(last) ? 1
    : last === "weekly" || WEEK_DURATION.test(last) ? 2
    : last === "monthly" ? 3 : 4;
  return {digitGroup, group, windowRank};
}

/** Parses documented statusLine quota JSON `{ quota: { [bucketId]: { remaining_fraction?, reset_time?, reset_in_seconds?, disabled? } }, plan_tier? }`. */
export function parseStatusLineQuota(raw:unknown, nowMs:number): ParsedQuotaReading | null {
  if (!Number.isFinite(nowMs) || nowMs < 0) return null;
  let root:unknown = raw;
  if (typeof raw === "string") {
    if (!raw || raw.length > 64 * 1024) return null;
    try {
      root = JSON.parse(raw);
    } catch {
      return null;
    }
  }
  if (!isPlainObject(root)) return null;
  const quota = root.quota;
  if (!isPlainObject(quota)) return null;
  const keys = Object.keys(quota);
  if (keys.length > MAX_QUOTA_BUCKETS) return null;
  const entries:{limit:QuotaLimit; digitGroup:number; group:string; windowRank:number}[] = [];
  for (let i = 0; i < keys.length; i++) {
    const rawKey = keys[i];
    const validKey = validBucketLabel(rawKey);
    if (!validKey) return null;
    const entry = quota[rawKey];
    if (!isPlainObject(entry)) return null;
    if (entry.disabled !== undefined && typeof entry.disabled !== "boolean") return null;
    if (entry.disabled === true) continue;
    const fraction = entry.remaining_fraction;
    if (fraction === undefined || fraction === null) continue;
    if (typeof fraction !== "number" || !Number.isFinite(fraction) || fraction < 0 || fraction > 1) return null;
    const reset = parseBucketResetAt(entry.reset_time, entry.reset_in_seconds, nowMs);
    if (!reset.ok) return null;
    const meta = bucketSortMeta(validKey);
    entries.push({
      limit:{
        label:formatBucketLabel(validKey),
        used_percent:Math.round((1 - fraction) * 1000) / 10,
        resets_at:reset.value,
      },
      ...meta,
    });
  }
  entries.sort((a, b) =>
    a.digitGroup - b.digitGroup
    || a.group.localeCompare(b.group)
    || a.windowRank - b.windowRank
    || b.limit.used_percent - a.limit.used_percent
    || a.limit.label.localeCompare(b.limit.label));
  const limits = entries.map(entry => entry.limit);
  const rawTier = root.plan_tier;
  const tier = typeof rawTier === "string" && rawTier.trim().length > 0 && rawTier.trim().length <= 64 && PRINTABLE_LABEL.test(rawTier.trim())
    ? rawTier.trim() : null;
  return {title:tier ? `Account quota (${tier})` : "Account quota", limits};
}

export function statusLineQuotaSnapshot(raw:unknown, nowMs:number): ProfileLimitsReply {
  const parsed = parseStatusLineQuota(raw, nowMs);
  if (!parsed) return {error:"Status line quota response has an unexpected format"};
  if (!parsed.limits.length) return {error:"Status line reported no active model quota"};
  return snapshot("subscription", {quota:parsed}, nowMs);
}

function statusLinePushQuota(): ProfileLimitsReply {
  return {error:"Status line quota updates arrive from the agent hook"};
}

/** Quota protocols this plugin implements, named by a profile's declared `reader`. */
const quotaReaders:Readonly<Record<string,(ports:LimitsPorts, nowMs:number) => ProfileLimitsReply>> = {
  "gemini-code-assist": geminiCodeAssistQuota,
  "statusline-quota": statusLinePushQuota,
};

const own = <T>(table:Readonly<Record<string,T>>, key:string):T | undefined =>
  Object.prototype.hasOwnProperty.call(table, key) ? table[key] : undefined;

/** Credentials never enter the plugin: the host fills `{secret}` and `{accessToken}` headers from what the manifest and profile declare. */
export function readProfileLimits(ports:LimitsPorts, request:ProfileLimitsRequest, nowMs:number): ProfileLimitsReply {
  const source = request.source;
  if (source.kind === "quota") {
    const reader = own(quotaReaders, source.reader);
    return reader ? reader(ports, nowMs) : {error:"Profile names an unknown quota reader"};
  }
  if (source.kind !== "balance") return {error:"Profile declares no plugin-read limits source"};
  const reader = own(readers, source.reader);
  if (!reader) return {error:"Profile names an unknown balance reader"};
  return readJson(ports.fetch, {
    url:reader.url, method:"GET", timeoutMs:15000, headers:{Accept:"application/json"},
    profileSecret:{profile:request.profile, env:source.credentialEnv, headers:{Authorization:"Bearer {secret}"}},
  }, "Balance", body => {
    const balance = reader.parse(body);
    return balance ? snapshot("api", {balance}, nowMs) : {error:"Balance response has an unexpected format"};
  });
}
