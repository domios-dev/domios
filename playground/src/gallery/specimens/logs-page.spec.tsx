import { LogsTab, type AppLog } from "../../../../src/components/Settings/LogsTab";
import { appConfig, httpApi, mockDaemon } from "../fixtures";
import { ClickOnce } from "../stageTools";
import { defineSpecimen } from "../types";

const MINUTE = 60_000;
const EXPANDED_ID = 3;

function log(id: number, minutesAgo: number, level: AppLog["level"], source: string, message: string, context?: unknown): AppLog {
  return { id, ts: Date.now() - minutesAgo * MINUTE, level, source, message, context_json: context === undefined ? null : JSON.stringify(context) };
}

const ROWS: AppLog[] = [
  log(1, 1, "info", "daemon", "API listening on 127.0.0.1:7686"),
  log(2, 4, "info", "cron", "Morning digest finished in 3m 42s"),
  log(EXPANDED_ID, 9, "error", "agent.spawn", "Codex exited with code 1 before the first prompt", { tab_id: "tab-7f2c", provider: "codex", model: "gpt-6-sol", exit_code: 1, stderr_tail: ["auth: token expired", "run `codex login` to continue"] }),
  log(4, 17, "warn", "mesh", "Peer home-laptop missed two heartbeats", { peer: "home-laptop", last_seen_ms: 48_000 }),
  log(5, 26, "info", "plan", "Plan release-1.14 moved to execution"),
  log(6, 41, "warn", "usage", "Claude session usage at 82%"),
  log(7, 63, "error", "bridge.telegram", "Send failed: 429 Too Many Requests", { retry_after_s: 12 }),
  log(8, 95, "info", "memory", "Librarian vectorized 14 new skills"),
];

const apis = {
  rows: httpApi("logs-rows"),
  expanded: httpApi("logs-expanded"),
  empty: httpApi("logs-empty"),
  failed: httpApi("logs-failed"),
};
mockDaemon(apis.rows, "/api/logs", () => ROWS);
mockDaemon(apis.expanded, "/api/logs", () => ROWS);
mockDaemon(apis.empty, "/api/logs", () => []);
mockDaemon(apis.failed, "/api/logs", () => new Response("", { status: 503 }));

function LogsSpecimen({ api, expand = false }: { api: ReturnType<typeof httpApi>; expand?: boolean }) {
  const page = <div className="gallery-fill gallery-panel"><LogsTab config={appConfig(api)} /></div>;
  return expand ? <ClickOnce selector={`[data-log-id="${EXPANDED_ID}"] [data-testid="settings-log-row-toggle"]`}>{page}</ClickOnce> : page;
}

export default defineSpecimen({
  id: "logs-page",
  title: "Logs page",
  category: "Settings",
  where: "src/components/Settings/LogsTab.tsx · LogsFilterBar.tsx · LogsListRow.tsx",
  stage: "full",
  notes: "The shipped Settings → Logs page over a mocked GET /api/logs on the daemon origin: level pills tinted by tone, the source field, the time-range Segmented and calm ct-row log rows. The expanded stage opens a row through its own toggle to show the stamp and pretty-printed context JSON.",
  Component: LogsSpecimen,
  states: [
    { id: "rows", label: "Info · warn · error rows", props: { api: apis.rows } },
    { id: "expanded", label: "Error row expanded · context JSON", props: { api: apis.expanded, expand: true } },
    { id: "empty", label: "No logs in range", props: { api: apis.empty } },
    { id: "failed", label: "Daemon unreachable · HTTP 503", props: { api: apis.failed } },
  ],
});
