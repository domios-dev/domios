import { localJson } from "../webApi";

type SecretItem = import("../../../gen/SecretItem").SecretItem;

/** Only entries the user owns can bind to a template's secret ask; Domios's own entries are never offered. */
export function userSecretNames(items: readonly Pick<SecretItem, "name" | "owner">[]): string[] {
  return [...new Set(items.flatMap((item) => item.owner === "user" ? [item.name] : []))].sort();
}

/** Names of the user's vault entries a secret ask can bind to; values never leave the vault. */
export async function listVaultSecretNames(): Promise<string[]> {
  const result = await localJson<{ items: SecretItem[] }>("GET", "/api/secrets/items?limit=200");
  return userSecretNames(result.items ?? []);
}

export function openSecretsSettings(): void {
  window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:secrets" } }));
}
