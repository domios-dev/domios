import { localJson } from "../webApi";

/** One `/api/secrets/items` row as far as a secret ask needs it; `owner` separates the user's entries from Domios's own. */
export interface VaultItem { name?: unknown; owner?: unknown }

/** Only entries the user owns can bind to a template's secret ask; an unmarked entry is treated as Domios's own. */
export function userSecretNames(items: readonly VaultItem[]): string[] {
  return [...new Set(items.flatMap((item) => item.owner === "user" && typeof item.name === "string" ? [item.name] : []))].sort();
}

/** Names of the user's vault entries a secret ask can bind to; values never leave the vault. */
export async function listVaultSecretNames(): Promise<string[]> {
  const result = await localJson<{ items?: VaultItem[] }>("GET", "/api/secrets/items?limit=200");
  return userSecretNames(result.items ?? []);
}

export function openSecretsSettings(): void {
  window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:secrets" } }));
}
