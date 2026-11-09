export type EmbedderIdentity = import("../../../../gen/EmbedderIdentity").EmbedderIdentity;

export type ReembedProgress = Pick<import("../../../../gen/ReembedProgress").ReembedProgress, "state" | "target" | "total" | "done" | "failed"> & Partial<import("../../../../gen/ReembedProgress").ReembedProgress>;

export type EmbedderStatus = Pick<import("../../../../gen/EmbedderStatus").EmbedderStatus, "provider" | "model" | "dimension" | "remote" | "available" | "index" | "index_matches" | "vectors" | "pending"> & Partial<Omit<import("../../../../gen/EmbedderStatus").EmbedderStatus, "reembed">> & { reembed: ReembedProgress };

export type Refusal = import("../../../../gen/EmbedderRefusal").EmbedderRefusal;

export function normalizeRefusal(reason: unknown): Refusal {
  if (typeof reason === "string") return { message: reason, hints: [] };
  if (reason !== null && typeof reason === "object") {
    return {
      message: "message" in reason && typeof reason.message === "string" ? reason.message : "The embedder request failed.",
      hints: "hints" in reason && Array.isArray(reason.hints)
        ? reason.hints.filter((hint): hint is string => typeof hint === "string")
        : [],
    };
  }
  return { message: "The embedder request failed.", hints: [] };
}

export type ProviderModel = import("../../../../gen/EmbedderModelDescriptor").EmbedderModelDescriptor;

export type ProviderListing = Omit<import("../../../../gen/ProviderListing").ProviderListing, "key" | "modalities"> & Partial<Pick<import("../../../../gen/ProviderListing").ProviderListing, "modalities">> & {key?: Omit<NonNullable<import("../../../../gen/ProviderListing").ProviderListing["key"]>, "vault_name"> | null};

export function refusalForPlatform(reason: unknown, provider: ProviderListing | undefined, remoteOptIn: boolean, platform: string): Refusal {
  const refusal = normalizeRefusal(reason);
  if (!provider?.remote) return refusal;
  const hints: string[] = [];
  const key = provider.key;
  if (!key || (key.file.present ? key.file.owner_only !== true : !key.in_vault)) {
    hints.push(`Pipe the key on stdin to: domios mem embedder key set ${provider.id}`);
    const file = key?.file.path ?? `<data_dir>/secrets/${provider.id}-embedder.key`;
    hints.push(/^win/i.test(platform)
      ? `Manual Windows fallback: create ${file} as an empty file, disable inheritance and grant an owner-only ACL with icacls before writing the key (domios docs MEMORY).`
      : `Manual Unix fallback: create ${file} with umask 077 and chmod 600 before writing the key (domios docs MEMORY).`);
  }
  if (!remoteOptIn) {
    hints.push("After reading the warning that memory text goes to a third party: domios config set memory.embedder.remote_opt_in true");
  }
  return normalizeRefusal({ message: refusal.message, hints });
}

export type EmbedderListing = Omit<import("../../../../gen/EmbedderListing").EmbedderListing, "providers" | "status"> & {providers: ProviderListing[]; status: EmbedderStatus};

export type SwitchPreview = Pick<import("../../../../gen/SwitchPreview").SwitchPreview, "target" | "remote" | "warning" | "total" | "refusal"> & Partial<Pick<import("../../../../gen/SwitchPreview").SwitchPreview, "already_active" | "already_running" | "cancelled">> & {plan: Pick<import("../../../../gen/SwitchPlan").SwitchPlan, "items" | "entities" | "docs"> & Partial<Pick<import("../../../../gen/SwitchPlan").SwitchPlan, "blueprints">>};

export interface EmbedderOption {
  value: string;
  provider: string;
  model: string;
  label: string;
  disabled: boolean;
  reason: string | null;
}

const SEPARATOR = "\u0000";

export function optionValue(provider: string, model: string): string {
  return `${provider}${SEPARATOR}${model}`;
}

export function parseOption(value: string): { provider: string; model: string } {
  const [provider, model = ""] = value.split(SEPARATOR);
  return { provider, model };
}

/** One option per provider model; a remote one is disabled while its key or the opt-in is missing. */
export function embedderOptions(listing: EmbedderListing): EmbedderOption[] {
  return listing.providers.flatMap((provider) =>
    provider.models.map((model) => ({
      value: optionValue(provider.id, model.model),
      provider: provider.id,
      model: model.model,
      label: `${provider.label} · ${model.model} (${model.dimension} dims)`,
      disabled: provider.refusal !== null,
      reason: provider.refusal?.message ?? null,
    })),
  );
}

export function activeOption(status: EmbedderStatus): string {
  return optionValue(status.provider, status.model);
}

export function progressValue(status: EmbedderStatus): number {
  const { done, total, current_item } = status.reembed;
  return Math.min(total, done + (current_item && current_item.total > 0 ? current_item.done / current_item.total : 0));
}

export function progressLabel(status: EmbedderStatus): string {
  const { state, done, total, failed, target, last_error, current_item } = status.reembed;
  if (state === "running") {
    const percent = total === 0 ? 100 : Math.floor((progressValue(status) * 100) / total);
    const to = target ? ` for ${target.provider}/${target.model}` : "";
    const chunks = current_item && current_item.total > 1
      ? `; current item ${current_item.done} of ${current_item.total} chunks`
      : "";
    return `Re-embedding ${done} of ${total} (${percent}%)${to}${chunks}`;
  }
  if (state === "completed") return failed > 0 ? `Re-embedded ${done - failed} of ${total}; ${failed} failed` : `Re-embedded ${total} items`;
  if (state === "failed" || state === "cancelled") {
    return `Re-embed ${state} at ${done} of ${total}: ${last_error ?? "no reason recorded"}`;
  }
  return status.index_matches ? "Index up to date" : "Index belongs to another model";
}

export function isReembedding(status: EmbedderStatus): boolean {
  return status.reembed.state === "running";
}

export function switchSummary(preview: SwitchPreview): string {
  const { items, entities, docs } = preview.plan;
  return `Switching re-embeds ${preview.total} items (${items} memories, ${entities} entities, ${docs} doc sections) in the background.`;
}

/** The status an event payload carries, or null when it is not one. */
export function statusFromEvent(payload: unknown): EmbedderStatus | null {
  if (!payload || typeof payload !== "object") return null;
  const candidate = payload as Partial<EmbedderStatus>;
  return typeof candidate.provider === "string" && typeof candidate.dimension === "number" && candidate.reembed
    ? (candidate as EmbedderStatus)
    : null;
}
