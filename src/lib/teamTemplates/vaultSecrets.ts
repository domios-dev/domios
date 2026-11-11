import { localJson } from "../webApi";

/** Names of the local vault entries a secret ask can bind to; values never leave the vault. */
export async function listVaultSecretNames(): Promise<string[]> {
  const result = await localJson<{ items?: { name?: unknown }[] }>("GET", "/api/secrets/items?limit=200");
  return [...new Set((result.items ?? []).flatMap((item) => typeof item.name === "string" ? [item.name] : []))].sort();
}

export function openSecretsSettings(): void {
  window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:secrets" } }));
}
