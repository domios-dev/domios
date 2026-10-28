import { parseDocument } from "yaml";
import { orchestrationTransport } from "../../lib/transport/orchestrationRuntime";

export interface BlueprintInput {
  type: "string" | "integer" | "number" | "boolean" | "object" | "array";
  description?: string;
  default?: unknown;
  enum?: unknown[];
}
export interface BlueprintRecipe {
  schema_version: 1;
  name: string;
  inputs?: Record<string, BlueprintInput>;
  watch?: Record<string, unknown>;
  pipeline?: Record<string, unknown>;
}
export interface BlueprintFile { name: string; yaml: string }

export function parseBlueprintRecipe(raw: string): BlueprintRecipe {
  if (new TextEncoder().encode(raw).length > 256 * 1024) throw new Error("Blueprint document exceeds 256 KiB");
  const parsed = parseDocument(raw, { version: "1.2", uniqueKeys: true });
  if (parsed.errors.length) throw new Error(parsed.errors.map((error) => error.message).join("\n"));
  const value: unknown = parsed.toJS({ maxAliasCount: 100 });
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid Blueprint document");
  const recipe = value as Record<string, unknown>;
  if (recipe.schema_version !== 1) throw new Error("Unsupported Blueprint schema version");
  if (typeof recipe.name !== "string" || !recipe.name.trim()) throw new Error("Blueprint name is required");
  if (Boolean(recipe.watch) === Boolean(recipe.pipeline)) throw new Error("Choose exactly one watch or pipeline");
  if (recipe.inputs !== undefined) {
    if (!recipe.inputs || typeof recipe.inputs !== "object" || Array.isArray(recipe.inputs)) throw new Error("Invalid Blueprint inputs");
    for (const input of Object.values(recipe.inputs)) {
      if (!input || typeof input !== "object" || Array.isArray(input) || !["string", "integer", "number", "boolean", "object", "array"].includes(String((input as Record<string, unknown>).type))) throw new Error("Invalid Blueprint input type");
    }
  }
  return recipe as unknown as BlueprintRecipe;
}

export interface BlueprintSharingApi {
  export(workspaceId: string, teamId: string | null, id: string): Promise<BlueprintFile>;
  /** `scope` names the destination ("general", "workspace:<id>", "team:<id>"); the caller tab must already live there. */
  import(callerTabId: string, source: string, assignments: string[], scope?: string): Promise<unknown>;
  save(file: BlueprintFile): void;
}

export const blueprintSharingApi: BlueprintSharingApi = {
  export: (workspaceId, teamId, id) => orchestrationTransport().blueprint_export_file({ workspaceId, teamId, id }) as Promise<BlueprintFile>,
  import: (callerTabId, source, assignments, scope) => orchestrationTransport().blueprint_import_file({ callerTabId, recipe: source, assignments, scope }),
  save: (file) => {
    const link = document.createElement("a");
    link.download = `${file.name.replace(/[^a-zA-Z0-9_-]+/g, "-") || "blueprint"}.yaml`;
    link.href = `data:application/yaml;charset=utf-8,${encodeURIComponent(file.yaml)}`;
    document.body.append(link);
    link.click();
    link.remove();
  },
};
