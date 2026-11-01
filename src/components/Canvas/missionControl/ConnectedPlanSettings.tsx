import { useGuideNavigation } from "@codeterm/chat-ui";
import type { ApiConfig } from "../../../lib/types";
import { InlineErrorNotice } from "../../shared/ErrorNotice";
import type { PlanControlLoop } from "../planControlLoop";
import type { TeamPlanSummary } from "./missionControlState";
import { PlanSettingsPanel } from "./PlanSettingsPanel";
import { planOversight } from "./planOversightAdapter";
import { usePlanControlLoop } from "./usePlanControlLoop";
import { usePlanSettingsClock } from "./usePlanSettingsClock";
import { openNotificationCenter } from "../../Notifications/openNotificationCenter";

export function ConnectedPlanSettings({ plan, api, initial, now }: {
  plan: TeamPlanSummary;
  api?: ApiConfig;
  initial?: PlanControlLoop;
  now?: number;
}) {
  const { value, busy, loadError, actionError, update, runNow } = usePlanControlLoop(api, plan.id, plan.updated_at, initial);
  const guide = useGuideNavigation();
  const clock = usePlanSettingsClock(now);
  const oversight = planOversight(plan, value, !api ? "disconnected" : loadError ? "unavailable" : "loading");
  return <>
    <PlanSettingsPanel plan={plan} api={api} oversight={oversight} now={clock}
      disabled={!api || !value || busy || plan.status !== "active" || plan.mode === "complete"}
      onModeChange={mode => { if (mode !== value?.mode) void update({ mode }); }}
      onIntervalChange={ms => void update({ mode: "heartbeat", heartbeat_interval_secs: ms === null ? null : ms / 1000 })}
      onRunNow={() => void runNow()}
      runPending={Boolean(value?.running || value?.manual_run_requested)}
      onOpenGuide={guide ? () => guide.onOpen("curator") : undefined} />
    {oversight.suspended_reason === "waiting_on_human" && <button type="button" className="mc-board-more" onClick={() => openNotificationCenter()}>Open pending questions</button>}
    {loadError && <InlineErrorNotice role="alert" error={loadError} operation="loading plan settings" />}
    {actionError && <InlineErrorNotice role="alert" description={{ title: "Curator could not run", cause: actionError, raw: actionError, actions: [] }} />}
  </>;
}
