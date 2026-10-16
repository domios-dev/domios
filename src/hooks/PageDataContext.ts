import { createContext } from "react";
import type { useConfigLoader } from "./useConfig";
import type { useShellProfilesLoader } from "./useShellProfiles";
import type { useWorkspaceCronsLoader } from "./useWorkspaceCrons";

export interface PageData {
  config: ReturnType<typeof useConfigLoader>;
  shells: ReturnType<typeof useShellProfilesLoader>;
  crons: ReturnType<typeof useWorkspaceCronsLoader>;
}

export const PageDataContext = createContext<PageData | null>(null);
