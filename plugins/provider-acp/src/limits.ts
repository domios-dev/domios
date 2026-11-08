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

const decimal = /^-?\d{1,15}(?:\.\d{1,8})?$/;
const currency = /^[A-Z]{3}$/;

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

/** Quota protocols this plugin implements, named by a profile's declared `reader`. */
const quotaReaders:Readonly<Record<string,(ports:LimitsPorts, nowMs:number) => ProfileLimitsReply>> = {
  "gemini-code-assist": geminiCodeAssistQuota,
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
