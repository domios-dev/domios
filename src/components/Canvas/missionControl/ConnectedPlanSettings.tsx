import { useGuideNavigation } from "@codeterm/chat-ui";
import type { ApiConfig } from "../../../lib/types";
import { InlineErrorNotice } from "../../shared/ErrorNotice";
import type { PlanControlLoop } from "../planControlLoop";
import type { TeamPlanSummary } from "./missionControlState";
import { PlanSettingsPanel } from "./PlanSettingsPanel";
import { planOversight } from "./planOversightAdapter";
import { usePlanControlLoop } from "./usePlanControlLoop";
import { usePlanSettingsClock } from "./usePlanSettingsClock";

export function ConnectedPlanSettings({ plan, api, initial, now }: {
  plan: TeamPlanSummary;
  api?: ApiConfig;
  initial?: PlanControlLoop;
  now?: number;
}) {
  const { value, busy, error, update } = usePlanControlLoop(api, plan.id, plan.updated_at, initial);
  const guide = useGuideNavigation();
  const clock = usePlanSettingsClock(now);
  const oversight = planOversight(plan, value, !api ? "disconnected" : error ? "unavailable" : "loading");
  return <>
    <PlanSettingsPanel plan={plan} api={api} oversight={oversight} now={clock}
      disabled={!api || !value || busy || plan.status !== "active" || plan.mode === "complete"}
      onModeChange={mode => { if (mode !== value?.mode) void update({ mode }); }}
      onIntervalChange={ms => void update({ mode: "heartbeat", heartbeat_interval_secs: ms === null ? null : ms / 1000 })}
      onOpenGuide={guide ? () => guide.onOpen("curator") : undefined} />
    {error && <div role="alert"><InlineErrorNotice error={error} operation="updating plan settings" /></div>}
  </>;
}
