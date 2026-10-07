export interface DomiosPreset {
  name: string;
  model: string;
  prompt: string;
  params?: {
    temperature?: number;
    top_p?: number;
    steps?: number;
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isOwned(value: unknown): boolean {
  return isRecord(value) && value["x-domios"] === true;
}

export function applyDomiosPresets(
  config: unknown,
  presets: DomiosPreset[],
): { config: object; changed: boolean; conflicts: string[] } {
  const base = isRecord(config) ? config : {};
  const existing = isRecord(base.agent) ? base.agent : {};
  const wanted = new Set(presets.map((preset) => preset.name));
  const agents = new Map(
    Object.entries(existing).filter(([name, entry]) => !isOwned(entry) || wanted.has(name)),
  );
  const conflicts: string[] = [];
  for (const preset of presets) {
    if (agents.has(preset.name) && !isOwned(agents.get(preset.name))) {
      conflicts.push(preset.name);
      continue;
    }
    const params = preset.params;
    agents.set(preset.name, {
      model: preset.model,
      prompt: preset.prompt,
      mode: "primary",
      ...(params?.temperature !== undefined ? { temperature: params.temperature } : {}),
      ...(params?.top_p !== undefined ? { top_p: params.top_p } : {}),
      ...(params?.steps !== undefined ? { steps: params.steps } : {}),
      "x-domios": true,
    });
  }
  const result = { ...base };
  if (agents.size > 0) result.agent = Object.fromEntries(agents);
  else delete result.agent;
  return {
    config: result,
    changed: JSON.stringify(base) !== JSON.stringify(result),
    conflicts,
  };
}
