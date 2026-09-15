import type { AnalyticsGroupBy, AnalyticsModelKey, AnalyticsSnapshot } from "@codeterm/plugin-sdk";
import { keyId, keyLabel } from "./keys";

export type Catalog = Partial<Record<AnalyticsGroupBy, AnalyticsSnapshot>>;

export interface FilterOption<T> {
  id: string;
  value: T;
  label: string;
  detail?: string;
  unnamed: boolean;
}

export function providerLabels(catalog: Catalog): Map<string, string> {
  const labels = new Map<string, string>();
  for (const group of catalog.provider_model?.tokens.groups ?? []) {
    if (group.key.id !== null) labels.set(group.key.id, group.label ?? group.key.id);
  }
  return labels;
}

/** A provider the ledger never named cannot be filtered by id, so it is not offered. */
export function providerOptions(catalog: Catalog): FilterOption<string>[] {
  return (catalog.provider_model?.tokens.groups ?? [])
    .filter((group) => group.key.id !== null)
    .map((group) => ({ id: keyId(group.key), value: group.key.id!, label: group.label ?? group.key.id!, unnamed: false }));
}

export function modelOptions(catalog: Catalog): FilterOption<AnalyticsModelKey>[] {
  const labels = providerLabels(catalog);
  return (catalog.provider_model?.tokens.groups ?? []).flatMap((group) =>
    (group.children ?? [])
      .filter((child) => child.key.provider)
      .map((child) => ({
        id: keyId(child.key),
        value: { provider: child.key.provider!, model: child.key.id },
        label: keyLabel(child.key, child.label),
        detail: labels.get(child.key.provider!) ?? child.key.provider!,
        unnamed: child.key.id === null,
      })),
  );
}

/** Workspace, team and plan keys, a null key included: "No plan" is real work, selectable like any other. */
export function recordOptions(snapshot: AnalyticsSnapshot | undefined): FilterOption<string | null>[] {
  return (snapshot?.tokens.groups ?? []).map((group) => ({
    id: keyId(group.key),
    value: group.key.id,
    label: keyLabel(group.key, group.label),
    unnamed: group.key.id === null,
  }));
}

/** Every key is "never recorded": offering it as a filter would only select the whole range. */
export function onlyUnrecorded(options: readonly FilterOption<string | null>[]): boolean {
  return options.length > 0 && options.every((option) => option.id.endsWith("~workspace_not_recorded"));
}
