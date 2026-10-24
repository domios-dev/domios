import { useEffect, useState, type SetStateAction } from "react";
import type { BlueprintSolutionsApi } from "../components/Jobs/blueprintSolutions";
import type { JobsFilter } from "../components/Jobs/jobsModel";
import { catalogEntry, type CatalogEntry } from "../components/Jobs/solutionCatalog";
import { navigationId, navigationObject, optionalNavigationId, parentWhenMissing } from "../lib/viewerNavigation";
import { useViewerNavigation } from "./useViewerNavigation";

export type JobSelection = { kind: "cron" | "scenario" | "blueprint"; id: string };
type JobsRoute = JobSelection | { kind: "solution"; id: string; workspaceId: string } | null;

function jobsFilter(value: unknown): JobsFilter | undefined {
  const filter = navigationObject(value);
  const workspaceId = optionalNavigationId(filter?.workspaceId);
  const teamId = optionalNavigationId(filter?.teamId);
  return workspaceId !== undefined && teamId !== undefined ? { workspaceId, teamId } : undefined;
}

export function useJobsFilterNavigation(initial: JobsFilter, restore: boolean, ready: boolean, workspaces: readonly { id: string }[], teams: readonly { id: string }[]) {
  const [filter, setFilter] = useViewerNavigation<JobsFilter>(`jobs-filter:${JSON.stringify(initial)}`, initial, jobsFilter, restore);
  useEffect(() => {
    if (!ready) return;
    setFilter((current) => {
      const workspaceId = parentWhenMissing(current.workspaceId, true, (id) => workspaces.some((workspace) => workspace.id === id));
      const teamId = parentWhenMissing(current.teamId, true, (id) => teams.some((team) => team.id === id));
      return workspaceId === current.workspaceId && teamId === current.teamId ? current : { workspaceId, teamId };
    });
  }, [ready, workspaces, teams, setFilter]);
  return [filter, setFilter] as const;
}

export function jobsRoute(value: unknown): JobsRoute | undefined {
  if (value === null) return null;
  const route = navigationObject(value);
  if (!route || !navigationId(route.id)) return undefined;
  if (route.kind === "solution" && navigationId(route.workspaceId)) return { kind: "solution", id: route.id, workspaceId: route.workspaceId };
  return route.kind === "cron" || route.kind === "scenario" || route.kind === "blueprint" ? { kind: route.kind, id: route.id } : undefined;
}

export function useJobsNavigation(key: string, initial: JobSelection | null, api: BlueprintSolutionsApi) {
  const [route, setRoute] = useViewerNavigation<JobsRoute>(`jobs:${key}`, initial, jobsRoute, initial === null);
  const [entry, setEntry] = useState<CatalogEntry | null>(null);
  const selection = route?.kind === "solution" ? null : route;
  const openedSolution = route?.kind === "solution" && entry?.solution.id === route.id && entry.solution.workspaceId === route.workspaceId ? entry : null;
  const setSelection = (next: SetStateAction<JobSelection | null>) => setRoute((current) => typeof next === "function" ? next(current?.kind === "solution" ? null : current) : next);
  const setOpenedSolution = (next: CatalogEntry | null) => {
    setEntry(next);
    setRoute(next ? { kind: "solution", id: next.solution.id, workspaceId: next.solution.workspaceId } : null);
  };
  useEffect(() => {
    if (route?.kind !== "solution" || openedSolution) return;
    let active = true;
    void api.list(route.workspaceId).then(async (solutions) => {
      if (!active) return;
      const solution = solutions.find((candidate) => candidate.id === route.id);
      if (!solution) { if (active) setRoute(null); return; }
      const stored = await api.document(solution.id);
      if (active) setEntry(catalogEntry({ ...solution, workspaceId: route.workspaceId }, stored));
    }).catch(() => { if (active) setRoute(null); });
    return () => { active = false; };
  }, [route, openedSolution, api, setRoute]);
  return { selection, setSelection, openedSolution, setOpenedSolution };
}

export function useMissingJobFallback(selection: JobSelection | null, ready: boolean, exists: boolean, clear: (next: null) => void) {
  useEffect(() => {
    if (selection && parentWhenMissing(selection, ready, () => exists) === null) clear(null);
  }, [selection, ready, exists, clear]);
}
