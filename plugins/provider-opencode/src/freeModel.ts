import type { ModelInfo } from "@codeterm/plugin-sdk";

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function finiteNonNegative(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null;
}

/** Whether a model belongs to OpenCode Zen and works without external provider auth or paid tokens. */
export function isFreeZenModel(id: string, metadata?: Record<string, unknown>): boolean {
  const slash = id.indexOf("/");
  if (slash <= 0) return false;
  const provider = id.slice(0, slash).toLowerCase();
  const leaf = id.slice(slash + 1).toLowerCase();
  if (provider !== "opencode" || !leaf) return false;

  const cost = record(metadata?.cost);
  if (cost) {
    const input = finiteNonNegative(cost.input);
    const output = finiteNonNegative(cost.output);
    if (input !== null && output !== null) {
      const extraPaid = ["cache_read", "cache_write", "reasoning"].some((key) => {
        const amount = finiteNonNegative(cost[key]);
        return amount !== null && amount > 0;
      });
      return input === 0 && output === 0 && !extraPaid;
    }
  }
  return /(?:^|[-_.])free$/.test(leaf);
}

function supportsToolCalling(metadata?: Record<string, unknown>): boolean {
  if (!metadata) return true;
  if (metadata.tool_call === false) return false;
  const capabilities = record(metadata.capabilities);
  if (capabilities?.toolcall === false) return false;
  return true;
}

/**
 * Orders live models so zero-auth free Zen models lead the catalogue.
 * Core's live default selects the first available model; authenticated users
 * keep any explicit or configured model choice.
 */
export function orderOpencodeModels(
  models: ModelInfo[],
  metadata: ReadonlyMap<string, Record<string, unknown>>,
): ModelInfo[] {
  const freeToolCapable: ModelInfo[] = [];
  const freeOther: ModelInfo[] = [];
  const rest: ModelInfo[] = [];
  for (const model of models) {
    const meta = metadata.get(model.id);
    if (isFreeZenModel(model.id, meta)) {
      const decorated: ModelInfo = model.badge ? model : { ...model, badge: "free" };
      if (supportsToolCalling(meta)) freeToolCapable.push(decorated);
      else freeOther.push(decorated);
    } else {
      rest.push(model);
    }
  }
  return [...freeToolCapable, ...freeOther, ...rest];
}
