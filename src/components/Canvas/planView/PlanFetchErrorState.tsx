import { planFetchShowsFullError } from "./planViewFetchGate";
import { BusyState } from "../../shared/BusyState";

interface PlanFetchErrorStateProps {
  loading?: boolean;
  error?: string | null;
  itemCount?: number;
  onRetry?: () => void;
  testId?: string;
  className?: string;
}

export function PlanFetchErrorState({
  loading = false,
  error = null,
  itemCount = 0,
  onRetry,
  testId = "plan-fetch-error",
  className = "",
}: PlanFetchErrorStateProps) {
  if (loading && itemCount === 0) {
    return (
      <div data-testid={`${testId}-loading`} className={`plan-fetch-state ${className}`.trim()}>
        <BusyState labelKey="planLoad" inline />
      </div>
    );
  }

  if (planFetchShowsFullError(loading, error, itemCount)) {
    return (
      <div
        data-testid={testId}
        className={`plan-fetch-state plan-fetch-error flex flex-col items-center gap-2 px-4 text-center text-sm ${className}`.trim()}
        style={{ color: "var(--ct-red)" }}
      >
        <p>{error}</p>
        {onRetry && (
          <button
            type="button"
            data-testid={`${testId}-retry`}
            onClick={() => void onRetry()}
            className="rounded border px-3 py-1 text-xs cursor-pointer"
            style={{
              background: "var(--ct-bg)",
              borderColor: "var(--ct-border)",
              color: "var(--ct-fg)",
            }}
          >
            Retry
          </button>
        )}
      </div>
    );
  }

  return null;
}

export function PlanFetchReconnectingBanner({
  reconnecting = false,
  itemCount = 0,
  testId = "plan-fetch-reconnecting",
  className = "",
}: {
  reconnecting?: boolean;
  itemCount?: number;
  testId?: string;
  className?: string;
}) {
  if (!reconnecting || itemCount === 0) return null;
  return (
    <div
      data-testid={testId}
      className={`plan-fetch-reconnecting text-xs ${className}`.trim()}
      style={{ color: "var(--ct-muted-fg)" }}
    >
      <BusyState labelKey="planReconnect" inline />
    </div>
  );
}
