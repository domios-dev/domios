import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ApiConfig } from "../../../lib/types";
import type { Plan } from "../types";
import { EVENT_LOG_PAGE, formatCount, type EventPager } from "./missionControlBoardModel";
import { fetchTeamMissionControlState, type TeamMissionControlEvent } from "./missionControlState";

export interface TeamMissionControlView {
  activePlan: Plan | null;
  /** Plans the team ran before, newest first. */
  archivedPlans: readonly Plan[];
  events: EventPager;
}

/** Same plans by id and revision keep the previous array, so the Board does not re-render on an unchanged read. */
function samePlans(a: readonly Plan[], b: readonly Plan[]): boolean {
  return a.length === b.length && a.every((plan, index) => plan.id === b[index]!.id && plan.updated_at === b[index]!.updated_at);
}

interface TeamRead {
  activePlan: Plan | null;
  archivedPlans: Plan[];
  pages: TeamMissionControlEvent[][];
  nextCursor: string | null;
}

/** Reads the team's Mission Control state and pages its event stream by cursor, fifty events at a time. */
export function useTeamMissionControl(
  api: ApiConfig | undefined,
  teamId: string | null | undefined,
  revision: string,
): TeamMissionControlView | null {
  const [read, setRead] = useState<TeamRead | null>(null);
  const loadingRef = useRef(false);

  useEffect(() => {
    if (!api || !teamId) {
      setRead(null);
      return;
    }
    const controller = new AbortController();
    void fetchTeamMissionControlState(api, teamId, { limit: EVENT_LOG_PAGE }, controller.signal)
      .then((state) => {
        const archived = [...state.archived_plans].sort((a, b) => b.updated_at - a.updated_at);
        setRead((previous) => ({
          activePlan:
            previous?.activePlan && state.active_plan && samePlans([previous.activePlan], [state.active_plan])
              ? previous.activePlan
              : state.active_plan,
          archivedPlans: previous && samePlans(previous.archivedPlans, archived) ? previous.archivedPlans : archived,
          pages: [state.events],
          nextCursor: state.next_cursor,
        }));
      })
      .catch(() => {
        if (!controller.signal.aborted) setRead(null);
      });
    return () => controller.abort();
  }, [api, teamId, revision]);

  const loadNext = useCallback(() => {
    if (!api || !teamId || !read?.nextCursor || loadingRef.current) return;
    loadingRef.current = true;
    const cursor = read.nextCursor;
    void fetchTeamMissionControlState(api, teamId, { limit: EVENT_LOG_PAGE, cursor })
      .then((state) => {
        setRead((previous) =>
          previous && previous.nextCursor === cursor
            ? { ...previous, pages: [...previous.pages, state.events], nextCursor: state.next_cursor }
            : previous,
        );
      })
      .catch(() => undefined)
      .finally(() => {
        loadingRef.current = false;
      });
  }, [api, teamId, read]);

  return useMemo(() => {
    if (!read) return null;
    const loaded = read.pages.reduce((sum, page) => sum + page.length, 0);
    return {
      activePlan: read.activePlan,
      archivedPlans: read.archivedPlans,
      events: {
        pageCount: read.pages.length + (read.nextCursor ? 1 : 0),
        page: (index) => read.pages[index] ?? null,
        load: (index) => {
          if (index === read.pages.length) loadNext();
        },
        caption: `${formatCount(loaded)}${read.nextCursor ? "+" : ""} ${loaded === 1 && !read.nextCursor ? "event" : "events"}`,
      },
    };
  }, [read, loadNext]);
}
