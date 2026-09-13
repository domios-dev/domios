export const ALL_FILTER = "all";

export interface SpendChartFilter {
  key: string;
  label: string;
}

export interface SpendChartSegment {
  key: string;
  label: string;
  tone: number;
  value: number;
}

export interface SpendChartDay {
  start: number;
  label: string;
  today: boolean;
  segments: SpendChartSegment[];
  total: number;
}

/** What the spend chart draws: days of stacked segments, narrowed by one filter. */
export interface SpendChartSource {
  filters: SpendChartFilter[];
  days(filter: string): SpendChartDay[];
}

export function startOfToday(): number {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  return today.getTime();
}

export function dayLabel(start: number): string {
  return new Date(start).toLocaleDateString("en-US", { weekday: "short" });
}
