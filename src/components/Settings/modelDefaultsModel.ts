import type { AppConfig } from "../../lib/types";
import type { HelpSectionId } from "../../help/types";

export interface ModelDefaultRow {
  key: string;
  subject: string;
  provider: string;
  model: string;
  replacedFrom?: string;
}

export function modelDefaultGuideSection(row: ModelDefaultRow): HelpSectionId {
  return row.key.startsWith("memory.") ? "memory" : "agents";
}

export function modelDefaultPatch(config: AppConfig, row: ModelDefaultRow, model: string): Partial<AppConfig> {
  if (row.key === `agents.${row.provider}.default_model`) {
    return { agents: { ...config.agents, [row.provider]: { ...config.agents?.[row.provider], default_model: model } } };
  }
  for (const key of ["extraction_model", "dreaming_model", "worker_model"] as const) {
    if (row.key === `memory.${key}`) return { memory: { ...config.memory, [key]: model } };
  }
  throw new Error("Unknown model default");
}

function workerAgent(configured: string | undefined, fallback: string): string {
  return configured?.trim() ? configured : fallback;
}

/** Mirrors the daemon's stored-default inventory, so Settings lists what it repairs. */
export function modelDefaultRows(config: AppConfig): ModelDefaultRow[] {
  const replaced = config.model_defaults?.replaced ?? {};
  const rows: ModelDefaultRow[] = [];
  const push = (key: string, subject: string, provider: string, model: string | undefined) => {
    const value = model?.trim();
    if (!value) return;
    const record = replaced[key];
    const replacedFrom = record && record.to.toLowerCase() === value.toLowerCase() ? record.from : undefined;
    rows.push({ key, subject, provider, model: value, replacedFrom });
  };
  for (const provider of Object.keys(config.agents ?? {}).sort()) {
    push(`agents.${provider}.default_model`, "Default model", provider, config.agents[provider]?.default_model);
  }
  const memory = config.memory;
  if (memory) {
    const fallback = config.general_agent?.agent_provider ?? "";
    const extraction = workerAgent(memory.extraction_agent, fallback);
    const dreaming = workerAgent(memory.dreaming_agent, fallback);
    push("memory.extraction_model", "Memory extraction", extraction, memory.extraction_model);
    push("memory.dreaming_model", "Memory dreaming", dreaming, memory.dreaming_model);
    const workerProvider = !memory.extraction_model?.trim() ? extraction : !memory.dreaming_model?.trim() ? dreaming : null;
    if (workerProvider) push("memory.worker_model", "Memory workers", workerProvider, memory.worker_model);
  }
  return rows;
}
