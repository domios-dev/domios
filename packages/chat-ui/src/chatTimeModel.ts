export const CHAT_TIME_GUTTER_BREAKPOINT = 900;

export function showChatTimeGutter(width: number): boolean {
  return Number.isFinite(width) && width >= CHAT_TIME_GUTTER_BREAKPOINT;
}

export function chatHorizontalInsets(width: number, composerInset: number, timestampWidth: number) {
  const timeGutter = showChatTimeGutter(width) ? timestampWidth + 8 : 0;
  return { composerInset, contentInset: composerInset + timeGutter, timeGutter };
}

export interface ChatTimeLabels { today: string; yesterday: string }
export interface ChatTimeContext {
  now: number;
  timeZone: string;
  locale?: string;
  labels: ChatTimeLabels;
}

const formats = new Map<string, Intl.DateTimeFormat>();
function formatter(locale: string | undefined, timeZone: string, options: Intl.DateTimeFormatOptions) {
  const key = JSON.stringify([locale, timeZone, options]);
  let format = formats.get(key);
  if (!format) {
    format = new Intl.DateTimeFormat(locale, { ...options, timeZone });
    formats.set(key, format);
  }
  return format;
}

export function chatLocalDay(timestamp: number, timeZone: string): number | null {
  if (!Number.isFinite(new Date(timestamp).getTime())) return null;
  const parts = formatter("en-US", timeZone, { year: "numeric", month: "numeric", day: "numeric" }).formatToParts(timestamp);
  const part = (type: string) => Number(parts.find((value) => value.type === type)?.value);
  return Date.UTC(part("year"), part("month") - 1, part("day"));
}

export function messageTimeLabels(timestamp: number, context: Pick<ChatTimeContext, "timeZone" | "locale"> & { now?: number }) {
  if (!Number.isFinite(new Date(timestamp).getTime())) return null;
  const year = (value: number) => formatter(context.locale, context.timeZone, { year: "numeric" }).format(value);
  const sameYear = context.now != null && year(context.now) === year(timestamp);
  return {
    time: formatter(context.locale, context.timeZone, { hour: "numeric", minute: "2-digit" }).format(timestamp),
    date: formatter(context.locale, context.timeZone, { dateStyle: "full" }).format(timestamp),
    shortDate: formatter(context.locale, context.timeZone, sameYear ? { day: "numeric", month: "short" } : { day: "numeric", month: "short", year: "numeric" }).format(timestamp),
    dateTime: new Date(timestamp).toISOString(),
  };
}

export function chatDaySeparators(timestamps: readonly (number | null)[], context: ChatTimeContext): ReadonlyMap<number, string> {
  const result = new Map<number, string>();
  const today = chatLocalDay(context.now, context.timeZone);
  let previous: number | null = null;
  timestamps.forEach((timestamp, index) => {
    if (timestamp == null) return;
    const day = chatLocalDay(timestamp, context.timeZone);
    if (day == null) return;
    if (day !== previous) {
      result.set(index, day === today ? context.labels.today
        : today != null && day === today - 86_400_000 ? context.labels.yesterday
        : formatter(context.locale, context.timeZone, { dateStyle: "long" }).format(timestamp));
    }
    previous = day;
  });
  return result;
}
