import { parseDocument } from "yaml";
import { orchestrationTransport } from "../../lib/transport/orchestrationRuntime";

export interface BlueprintInput {
  type: "string" | "integer" | "number" | "boolean" | "object" | "array";
  description?: string;
  default?: unknown;
  enum?: unknown[];
}
/** What the import form reads from a recipe: a name to show and the inputs to bind. Every other field, and whether the
 *  recipe is valid at all, is the runtime's to judge when it imports. */
export interface BlueprintRecipe {
  name?: string;
  inputs?: Record<string, BlueprintInput>;
}
export interface BlueprintFile { name: string; yaml: string }

const INPUT_TYPES: readonly string[] = ["string", "integer", "number", "boolean", "object", "array"];

/** A light read before import: the file must be a YAML or JSON mapping within 256 KiB. Schema versions and shapes are not
 *  checked here; the import sends the file's bytes and the runtime refuses what it cannot accept, with its own reason. */
export function parseBlueprintRecipe(raw: string): BlueprintRecipe {
  if (new TextEncoder().encode(raw).length > 256 * 1024) throw new Error("Blueprint document exceeds 256 KiB");
  const parsed = parseDocument(raw, { version: "1.2", uniqueKeys: true });
  if (parsed.errors.length) throw new Error(parsed.errors.map((error) => error.message).join("\n"));
  const value: unknown = parsed.toJS({ maxAliasCount: 100 });
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid Blueprint document");
  const recipe = value as Record<string, unknown>;
  // Only inputs the form can bind are offered; anything else is left for the runtime to explain.
  const inputs = recipe.inputs && typeof recipe.inputs === "object" && !Array.isArray(recipe.inputs)
    ? Object.fromEntries(Object.entries(recipe.inputs).filter(([, input]) => input && typeof input === "object" && !Array.isArray(input)
      && INPUT_TYPES.includes(String((input as Record<string, unknown>).type)))) as Record<string, BlueprintInput>
    : undefined;
  return { ...(typeof recipe.name === "string" ? { name: recipe.name } : {}), ...(inputs && Object.keys(inputs).length ? { inputs } : {}) };
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
