import { invoke } from "../../lib/bridge";

export interface BlueprintRecipe {
  schema_version: number;
  placeholders: Record<string, string>;
  definition: Record<string, unknown>;
}

export function parseBlueprintRecipe(raw: string): BlueprintRecipe {
  if (new TextEncoder().encode(raw).length > 256 * 1024) throw new Error("Blueprint recipe exceeds 256 KiB");
  const value: unknown = JSON.parse(raw);
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid Blueprint recipe");
  const recipe = value as Record<string, unknown>;
  if (recipe.schema_version !== 1) throw new Error("Unsupported Blueprint schema version");
  if (!recipe.placeholders || typeof recipe.placeholders !== "object" || Array.isArray(recipe.placeholders)
    || Object.values(recipe.placeholders).some((description) => typeof description !== "string" || !description.trim())) throw new Error("Invalid placeholder descriptions");
  if (!recipe.definition || typeof recipe.definition !== "object" || Array.isArray(recipe.definition)) throw new Error("Invalid recipe definition");
  return recipe as unknown as BlueprintRecipe;
}

export interface BlueprintSharingApi {
  export(workspaceId: string, teamId: string | null, id: string): Promise<BlueprintRecipe>;
  import(callerTabId: string, recipe: BlueprintRecipe, assignments: string[]): Promise<unknown>;
  save(recipe: BlueprintRecipe): void;
}

export const blueprintSharingApi: BlueprintSharingApi = {
  export: (workspaceId, teamId, id) => invoke("blueprint_export_file", { workspaceId, teamId, id }),
  import: (callerTabId, recipe, assignments) => invoke("blueprint_import_file", { callerTabId, recipe, assignments }),
  save: (recipe) => {
    const link = document.createElement("a");
    const source = recipe.definition.create ?? recipe.definition.spec;
    const name = source && typeof source === "object" && "name" in source && typeof source.name === "string" ? source.name : "blueprint";
    link.download = `${name.replace(/[^a-zA-Z0-9_-]+/g, "-") || "blueprint"}.blueprint.json`;
    link.href = `data:application/json;charset=utf-8,${encodeURIComponent(JSON.stringify(recipe, null, 2))}`;
    document.body.append(link);
    link.click();
    link.remove();
  },
};
