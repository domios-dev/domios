import type {FetchOpts, FetchResult, RuntimeProfileLimitsSource} from "@codeterm/plugin-sdk";

export interface ProfileLimitsRequest {profile:string; source:RuntimeProfileLimitsSource}
export type FetchPort = <T>(opts:FetchOpts, then:(result:FetchResult) => T) => T;
export interface BalanceAmount {currency:string; amount:string}
/** Failures name their cause and never carry a response body. */
export interface ProfileLimitsFailure {error:string}
export type ProfileLimitsReply = string | ProfileLimitsFailure;

interface BalanceReader {
  url:string;
  parse(body:string): {available:boolean; amounts:BalanceAmount[]} | null;
}

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

function snapshot(balance:{available:boolean; amounts:BalanceAmount[]}, nowMs:number): string {
  return JSON.stringify({
    provider:"", account_id:null, captured_at_ms:nowMs, billing_mode:"api", limits_reported:true,
    session_pct:null, session_resets_at_ms:null, weekly_pct:null, weekly_resets_at_ms:null,
    spend_cents:null, spend_limit_cents:null, spend_remaining_cents:null, spend_resets_at_ms:null, raw_text:"",
    usage_details:{limits:[], dollar_credits:[], extra_usage:null, balance},
  });
}

const unexpectedResponse:ProfileLimitsFailure = {error:"Balance response has an unexpected format"};

/** The key never enters the plugin: the host fills the `{secret}` header from the profile's declared credential. */
export function readProfileLimits(fetch:FetchPort, request:ProfileLimitsRequest, nowMs:number): ProfileLimitsReply {
  const source = request.source;
  if (source.kind !== "balance") return {error:"Profile declares no balance source"};
  const reader = Object.prototype.hasOwnProperty.call(readers, source.reader) ? readers[source.reader] : undefined;
  if (!reader) return {error:"Profile names an unknown balance reader"};
  return fetch<ProfileLimitsReply>({
    url:reader.url, method:"GET", timeoutMs:15000, headers:{Accept:"application/json"},
    profileSecret:{profile:request.profile, env:source.credentialEnv, headers:{Authorization:"Bearer {secret}"}},
  }, result => {
    if (result.error) return {error:result.error};
    if (result.status !== 200) return {error:`Balance endpoint answered HTTP ${result.status}`};
    if (!result.body || result.body.length > 64 * 1024) return {error:"Balance response is empty or too large"};
    try {
      const balance = reader.parse(result.body);
      return balance ? snapshot(balance, nowMs) : unexpectedResponse;
    } catch {
      return unexpectedResponse;
    }
  });
}
