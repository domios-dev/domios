export interface SpendChartSeries {
  key: string;
  label: string;
  tone: number;
}

export interface SpendChartSegment extends SpendChartSeries {
  value: number;
}

export interface SpendChartDay {
  start: number;
  label: string;
  today: boolean;
  segments: SpendChartSegment[];
  total: number;
}

/** What the spend chart draws: a bar per day, and a legend only when the bars are split. */
export interface SpendChartData {
  days: SpendChartDay[];
  legend: SpendChartSeries[];
}

export function startOfToday(): number {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  return today.getTime();
}

export function dayLabel(start: number): string {
  return new Date(start).toLocaleDateString("en-US", { weekday: "short" });
}
