/** The chat-automation scene's report cards, in order: when each lands, in scene seconds. */
export const REPORT_COUNT = 7;
export const reportAt = (index: number) => 0.2 + index * 0.66;
