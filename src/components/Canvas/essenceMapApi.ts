import { invoke } from "../../lib/bridge";
import type { EssenceMapFile } from "./essenceMapModel";

export function fetchWorkspaceEssenceMaps(projectPaths: string[]): Promise<EssenceMapFile[]> {
  return invoke<EssenceMapFile[]>("read_workspace_essence_maps", { projectPaths });
}

export function fetchEssenceSource(projectPath: string, sourcePath: string): Promise<string> {
  return invoke<string>("read_workspace_essence_source", { projectPath, sourcePath });
}
