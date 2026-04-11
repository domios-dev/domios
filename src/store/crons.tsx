import {
  createContext,
  useContext,
  useCallback,
  useState,
  useEffect,
  useMemo,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { CronJob, CronRunInfo } from "../lib/types";

// ── Constants (easy to adjust) ───────────────────────────────────

/** How many runs to show collapsed in the sidebar */
export const CRON_SIDEBAR_RUNS_VISIBLE = 8;
/** Max run history stored per cron (backend default, also enforced here for display) */
export const CRON_MAX_HISTORY = 100;

// ── Context ──────────────────────────────────────────────────────

interface CronContextValue {
  crons: CronJob[];
  loading: boolean;
  /** Set of pane IDs owned by cron runs — used to filter them from the regular tab list */
  cronPaneIds: Set<string>;
  /** Run history keyed by cron job ID */
  runHistory: Record<string, CronRunInfo[]>;
  reload: () => void;
  create: (params: {
    name: string;
    workspaceId: string;
    schedule: string;
    task: string;
    agent?: string;
    /** Model id within the agent provider. `null` = use provider default. */
    model?: string | null;
    cwd?: string;
    description?: string;
    oneshot?: boolean;
  }) => Promise<CronJob>;
  update: (
    id: string,
    patch: {
      name?: string;
      schedule?: string;
      task?: string;
      cwd?: string;
      description?: string;
      enabled?: boolean;
      agent?: string;
      model?: string | null;
    },
  ) => Promise<CronJob>;
  remove: (id: string) => Promise<void>;
  toggle: (id: string, enabled: boolean) => Promise<CronJob>;
  trigger: (id: string) => Promise<CronRunInfo>;
  history: (id: string) => Promise<CronRunInfo[]>;
}

const CronContext = createContext<CronContextValue | null>(null);

export function CronProvider({ children }: { children: ReactNode }) {
  const [crons, setCrons] = useState<CronJob[]>([]);
  const [loading, setLoading] = useState(true);
  const [runHistory, setRunHistory] = useState<Record<string, CronRunInfo[]>>({});

  const loadAllHistory = useCallback(async (jobs: CronJob[]) => {
    const entries: Record<string, CronRunInfo[]> = {};
    for (const job of jobs) {
      try {
        const runs = await invoke<CronRunInfo[]>("cron_history", { id: job.id });
        entries[job.id] = Array.isArray(runs) ? runs : [];
      } catch {
        entries[job.id] = [];
      }
    }
    setRunHistory(entries);
  }, []);

  const reload = useCallback(() => {
    invoke<CronJob[]>("cron_list", {}).then((jobs) => {
      const list = Array.isArray(jobs) ? jobs : [];
      setCrons(list);
      setLoading(false);
      loadAllHistory(list);
    }).catch(() => setLoading(false));
  }, [loadAllHistory]);

  useEffect(() => {
    // Initial load — retry after delay if first call fails (backend may not be ready)
    reload();
    const retryTimer = setTimeout(() => reload(), 2000);
    const unsub1 = listen("cron-state-changed", () => reload());
    const unsub2 = listen("cron-fired", () => reload());
    // Also reload when mux state changes (workspaces restored)
    const unsub3 = listen("mux-state-changed", () => reload());
    return () => {
      clearTimeout(retryTimer);
      unsub1.then((fn) => fn());
      unsub2.then((fn) => fn());
      unsub3.then((fn) => fn());
    };
  }, [reload]);

  // Collect all pane IDs owned by cron runs
  const cronPaneIds = useMemo(() => {
    const ids = new Set<string>();
    for (const runs of Object.values(runHistory)) {
      for (const run of runs) {
        if (run.pane_id) ids.add(run.pane_id);
      }
    }
    // Also check last_run on each cron
    for (const job of crons) {
      if (job.last_run?.pane_id) ids.add(job.last_run.pane_id);
    }
    return ids;
  }, [runHistory, crons]);

  const create = useCallback(
    async (params: {
      name: string;
      workspaceId: string;
      schedule: string;
      task: string;
      agent?: string;
      model?: string | null;
      cwd?: string;
      description?: string;
      oneshot?: boolean;
    }) => {
      const result = await invoke<CronJob>("cron_create", {
        name: params.name,
        workspaceId: params.workspaceId,
        schedule: params.schedule,
        task: params.task,
        agent: params.agent,
        model: params.model ?? null,
        cwd: params.cwd,
        description: params.description,
        oneshot: params.oneshot,
      });
      reload();
      return result;
    },
    [reload],
  );

  const update = useCallback(
    async (
      id: string,
      patch: {
        name?: string;
        schedule?: string;
        task?: string;
        cwd?: string;
        description?: string;
        enabled?: boolean;
        agent?: string;
        model?: string | null;
      },
    ) => {
      const result = await invoke<CronJob>("cron_update", { id, ...patch });
      reload();
      return result;
    },
    [reload],
  );

  const remove = useCallback(
    async (id: string) => {
      await invoke("cron_delete", { id });
      reload();
    },
    [reload],
  );

  const toggle = useCallback(
    async (id: string, enabled: boolean) => {
      const result = await invoke<CronJob>("cron_toggle", { id, enabled });
      reload();
      return result;
    },
    [reload],
  );

  const triggerFn = useCallback(
    async (id: string) => {
      const result = await invoke<CronRunInfo>("cron_trigger", { id });
      reload();
      return result;
    },
    [reload],
  );

  const historyFn = useCallback(async (id: string) => {
    return invoke<CronRunInfo[]>("cron_history", { id });
  }, []);

  return (
    <CronContext.Provider
      value={{
        crons,
        loading,
        cronPaneIds,
        runHistory,
        reload,
        create,
        update,
        remove,
        toggle,
        trigger: triggerFn,
        history: historyFn,
      }}
    >
      {children}
    </CronContext.Provider>
  );
}

export function useCrons() {
  const ctx = useContext(CronContext);
  if (!ctx) throw new Error("useCrons must be used within CronProvider");
  return ctx;
}
