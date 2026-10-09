import type { ApiConfig } from "../../lib/types";
import type { SupporterStatus } from "../Settings/useDecisionToken";
import { ActivityFeed } from "./ActivityFeed";
import { useSupporterResource } from "./useSupporterResource";
import { SupporterAllowanceRow } from "../Usage/SupporterAllowanceRow";
import { UI_TEXT } from "../shared/design";

export function AnalyticsSupporterSection({ api }: { api: ApiConfig }) {
  const { data: status } = useSupporterResource<SupporterStatus>(api, "/api/supporter/status");
  if (!status?.connected) return null;
  return (
    <details
      data-testid="analytics-supporter-activity"
      className="shrink-0 bg-[var(--ct-bg)]"
    >
      <summary className="cursor-pointer px-4 py-2">
        <span className={UI_TEXT.sectionLabel}>Spinal Cord activity</span>
      </summary>
      <div className="flex max-h-[40vh] flex-col gap-2 overflow-y-auto px-4 pb-3">
        <SupporterAllowanceRow api={api} />
        <ActivityFeed api={api} embedded />
      </div>
    </details>
  );
}
