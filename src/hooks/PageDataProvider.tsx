import { useLayoutEffect, type ReactNode } from "react";
import { PageDataContext } from "./PageDataContext";
import { useConfigLoader } from "./useConfig";
import { useShellProfilesLoader } from "./useShellProfiles";
import { useWorkspaceCronsLoader } from "./useWorkspaceCrons";
import { startPageGraphCache } from "../components/Automation/hostAdapter";

export function PageDataProvider({ children }: { children: ReactNode }) {
  const config = useConfigLoader();
  const shells = useShellProfilesLoader();
  const crons = useWorkspaceCronsLoader();
  useLayoutEffect(startPageGraphCache, []);
  return <PageDataContext.Provider value={{ config, shells, crons }}>{children}</PageDataContext.Provider>;
}
