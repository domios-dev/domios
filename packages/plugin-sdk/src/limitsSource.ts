import type {OptionalWireFields} from "./wireViews";
/** Where a launch profile's account limits come from; hosts branch only on `kind`. */
export type RuntimeProfileLimitsSource = import("./gen/RuntimeProfileLimitsSource").RuntimeProfileLimitsSource;

export const maxUnavailableReasonChars = 240;

interface CredentialDeclarations {secretEnv?:Readonly<Record<string,unknown>>; authEnv?:Readonly<Record<string,unknown>>}

function exactKeys(value:object, keys:readonly string[]): boolean {
  const own = Object.keys(value);
  return own.length === keys.length && own.every(key => keys.includes(key));
}
function providerIdentity(value:unknown): boolean {
  return typeof value === "string" && value.length <= 64 && /^[a-z0-9][a-z0-9_-]*$/.test(value)
    && !/^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(value);
}
function readerIdentity(value:unknown): boolean {
  return typeof value === "string" && /^[a-z][a-z0-9-]{0,63}$/.test(value);
}
function reason(value:unknown): boolean {
  if (typeof value !== "string" || !value.trim() || value.trim() !== value) return false;
  return [...value].length <= maxUnavailableReasonChars && !/\p{Cc}/u.test(value);
}
const owns = (map:Readonly<Record<string,unknown>> | undefined, key:string) => !!map && Object.prototype.hasOwnProperty.call(map, key);

/** Declarations a profile installed from a registry id inherits unless it declares its own. */
export type RuntimeRegistryDefaults = OptionalWireFields<import("./gen/RuntimeRegistryDefaults").RuntimeRegistryDefaults, "limitsSource" | "fullAccessMode">;

/** An agent session mode id a launch profile declares; bounded and free of control characters. */
export function validSessionMode(mode:unknown): mode is string {
  return typeof mode === "string" && Array.from(mode).length >= 1 && Array.from(mode).length <= 128 && !/[\u0000-\u001f\u007f-\u009f]/.test(mode);
}

/** Keyed by registry id; a default must hold for a credential-free profile, since registry installs carry none. */
export function validRegistryDefaults(defaults:unknown): defaults is Readonly<Record<string,RuntimeRegistryDefaults>> {
  if (!defaults || typeof defaults !== "object" || Array.isArray(defaults)) return false;
  const entries = Object.entries(defaults);
  return entries.length <= 128 && entries.every(([id, entry]) =>
    /^[a-z][a-z0-9-]{0,127}$/.test(id) && !!entry && typeof entry === "object" && !Array.isArray(entry)
    && Object.keys(entry).every(key => key === "limitsSource" || key === "fullAccessMode")
    && ((entry as RuntimeRegistryDefaults).limitsSource === undefined || validLimitsSource((entry as RuntimeRegistryDefaults).limitsSource, {}))
    && ((entry as RuntimeRegistryDefaults).fullAccessMode === undefined || validSessionMode((entry as RuntimeRegistryDefaults).fullAccessMode)));
}

/** The same rules the host manifest validator applies, so plugins and the host agree on shared fixtures. */
export function validLimitsSource(source:unknown, profile:CredentialDeclarations): source is RuntimeProfileLimitsSource {
  if (!source || typeof source !== "object" || Array.isArray(source)) return false;
  const value = source as Record<string,unknown>;
  switch (value.kind) {
    case "account": return exactKeys(value, ["kind","provider"]) && providerIdentity(value.provider);
    case "balance": return exactKeys(value, ["kind","reader","credentialEnv"]) && readerIdentity(value.reader)
      && typeof value.credentialEnv === "string" && (owns(profile.secretEnv, value.credentialEnv) || owns(profile.authEnv, value.credentialEnv));
    case "quota": return exactKeys(value, ["kind","reader"]) && readerIdentity(value.reader);
    case "unavailable": return exactKeys(value, ["kind","reason"]) && reason(value.reason);
    default: return false;
  }
}
