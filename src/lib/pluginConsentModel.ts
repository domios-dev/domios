export interface SensitivePermission {
  key: string;
  label: string;
  allowed: boolean;
  decided?: boolean;
}

export interface ConsentPlugin {
  id: string;
  display_name?: string | null;
  version?: string | null;
  network_requested?: boolean;
  sensitive_permissions?: SensitivePermission[] | null;
}

export type GrantRequest = { pluginId: string; pluginName: string; permission: string; label: string };

export function installConsentRequests(plugin: ConsentPlugin): GrantRequest[] {
  return permissionRequests(plugin, (permission) => !permission.decided);
}

export function missingPermissions(plugin: ConsentPlugin): GrantRequest[] {
  return permissionRequests(plugin, (permission) => !permission.allowed);
}

function permissionRequests(plugin: ConsentPlugin, needsConsent: (permission: SensitivePermission) => boolean): GrantRequest[] {
  return (plugin.sensitive_permissions ?? []).filter(needsConsent).map((permission) => ({
    pluginId: plugin.id, pluginName: plugin.display_name ?? plugin.id,
    permission: permission.key, label: permission.label,
  }));
}

export function requiredGrant(error: unknown, plugin: ConsentPlugin): GrantRequest | null {
  const text = permissionErrorText(error);
  const deniedHost = /(?:fetch(?:\.async|\.stream)? denied:|network_grant_required:)\s*([a-zA-Z0-9.[\]:-]+)/.exec(text)?.[1];
  if (plugin.network_requested && deniedHost && validConsentHost(deniedHost)) {
    return { pluginId: plugin.id, pluginName: plugin.display_name ?? plugin.id,
      permission: `network:${deniedHost}`, label: `Connect to ${deniedHost}` };
  }
  return permissionRequests(plugin, () => true).find((permission) =>
    new RegExp(`\\b${permission.permission}_grant_required\\b`).test(text),
  ) ?? null;
}

export async function runWithConsent<T>(operation: () => Promise<T>, plugin: ConsentPlugin,
  consent: (request: GrantRequest) => Promise<boolean>, grant: (request: GrantRequest) => Promise<void>): Promise<T> {
  let failure: unknown;
  try {
    const result = await operation();
    if (!requiredGrant(result, plugin) && !hasGrantFailure(result)) return result;
    failure = result;
  } catch (error) { failure = error; }
  const request = requiredGrant(failure, plugin);
  if (!request) {
    if (hasGrantFailure(failure)) throw new Error("This plugin permission is unavailable. Check plugin Permissions.");
    throw failure;
  }
  if (!await consent(request)) throw new Error("Permission was not allowed.");
  await grant(request);
  try {
    const result = await operation();
    if (requiredGrant(result, plugin) || hasGrantFailure(result)) throw new Error("Permission is still unavailable. Check plugin permissions and try again.");
    return result;
  } catch (error) {
    if (requiredGrant(error, plugin) || hasGrantFailure(error)) throw new Error("Permission is still unavailable. Check plugin permissions and try again.");
    throw error;
  }
}

function validConsentHost(host: string): boolean {
  try {
    const url = new URL(`https://${host}`);
    return Boolean(url.hostname) && !url.username && !url.password && url.pathname === "/";
  } catch { return false; }
}

function permissionErrorText(value: unknown): string {
  return value instanceof Error ? value.message : typeof value === "string" ? value :
    value && typeof value === "object" ? JSON.stringify(value) : "";
}

function hasGrantFailure(value: unknown): boolean {
  return /\b[a-z_]+_grant_required\b/.test(permissionErrorText(value));
}
