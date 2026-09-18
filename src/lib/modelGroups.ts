/**
 * Model-group resolution for the spawn picker. The provider declares the groups
 * and their usage; these helpers only match a model id to a group by prefix, so
 * core never maps a vendor id to a label or a usage source.
 */
import type { ModelGroup, ModelGroupUsage } from "./types";

/** The segment before the first `/` — the generic model-id prefix a provider's
 *  group ids are expected to match when no group is declared. */
export function modelIdPrefix(modelId: string): string {
  const slash = modelId.indexOf("/");
  return slash > 0 ? modelId.slice(0, slash) : modelId;
}

/** The declared group a model belongs to: the longest group id that names the
 *  model's prefix (`deepseek` matches `deepseek/...`). `null` when none match. */
export function modelGroupForId(
  groups: ModelGroup[] | undefined,
  modelId: string,
): ModelGroup | null {
  let best: ModelGroup | null = null;
  for (const group of groups ?? []) {
    if (modelId === group.id || modelId.startsWith(`${group.id}/`)) {
      if (!best || group.id.length > best.id.length) best = group;
    }
  }
  return best;
}

/** Usage the provider reported for a model's group. Matches the declared group
 *  id first, then the bare id prefix, so a provider that reports usage without
 *  declaring a label still surfaces it. `null` when the group has no entry. */
export function usageForModel(
  usages: readonly ModelGroupUsage[],
  groups: ModelGroup[] | undefined,
  modelId: string,
): ModelGroupUsage | null {
  const group = modelGroupForId(groups, modelId);
  const key = group?.id ?? modelIdPrefix(modelId);
  return usages.find((usage) => usage.group === key) ?? null;
}

/** True when a usage entry carries anything worth showing. A group that is
 *  connected but unused reports zeroes and renders nothing. */
export function hasModelUsage(usage: ModelGroupUsage | null): usage is ModelGroupUsage {
  if (!usage) return false;
  return (
    usage.messages > 0 ||
    usage.input_tokens > 0 ||
    usage.output_tokens > 0 ||
    usage.cache_read_tokens > 0 ||
    usage.cache_write_tokens > 0 ||
    usage.cost_micros > 0
  );
}

export function formatModelCost(costMicros: number): string {
  const dollars = costMicros / 1_000_000;
  if (dollars > 0 && dollars < 0.01) return "<$0.01";
  return `$${dollars.toFixed(2)}`;
}

export function formatModelTokens(usage: ModelGroupUsage): string {
  const total =
    usage.input_tokens +
    usage.output_tokens +
    usage.cache_read_tokens +
    usage.cache_write_tokens;
  return `${formatCompactTokens(total)} tok`;
}

function formatCompactTokens(tokens: number): string {
  if (tokens >= 1_000_000_000) return `${(tokens / 1_000_000_000).toFixed(1)}B`;
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}K`;
  return String(tokens);
}

/** Short trailing label for a model row, e.g. `$1.94 · 4.8M tok`. */
export function formatModelUsage(usage: ModelGroupUsage): string {
  return `${formatModelCost(usage.cost_micros)} · ${formatModelTokens(usage)}`;
}
