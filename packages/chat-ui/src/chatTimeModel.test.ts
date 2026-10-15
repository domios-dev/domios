import assert from "node:assert/strict";
import { chatDaySeparators, messageTimeLabels, showChatTimeGutter, chatHorizontalInsets } from "./chatTimeModel";

const context = { now: Date.parse("2026-03-09T04:30:00Z"), timeZone: "America/New_York", locale: "en-GB", labels: { today: "current-day", yesterday: "previous-day" } };
assert.equal(showChatTimeGutter(899), false);
assert.equal(showChatTimeGutter(900), true);
assert.equal(showChatTimeGutter(1200), true);
assert.equal(showChatTimeGutter(Number.NaN), false);
assert.deepEqual(chatHorizontalInsets(899, 20, 28), { composerInset: 20, contentInset: 20, timeGutter: 0 });
assert.deepEqual(chatHorizontalInsets(900, 20, 28), { composerInset: 20, contentInset: 20, timeGutter: 36 });
assert.deepEqual(chatHorizontalInsets(1700, 402, 34), { composerInset: 20, contentInset: 20, timeGutter: 42 });
for (const composerInset of [0, 16, 20, 52, 402]) {
  for (const timestampWidth of [0, 28, 80]) {
    const insets = chatHorizontalInsets(1700, composerInset, timestampWidth);
    assert.equal(insets.contentInset, 20);
    assert.equal(insets.composerInset, 20);
  }
}
assert.deepEqual([...chatDaySeparators([
  Date.parse("2026-03-07T18:00:00Z"),
  Date.parse("2026-03-08T05:30:00Z"),
  null,
  Date.parse("2026-03-09T03:30:00Z"),
  Date.parse("2026-03-09T04:30:00Z"),
  Number.NaN,
], context)], [[0, "7 March 2026"], [1, "previous-day"], [4, "current-day"]]);
assert.deepEqual([...chatDaySeparators([], context)], []);
assert.deepEqual([...chatDaySeparators([null, Number.NaN], context)], []);
assert.deepEqual(messageTimeLabels(Date.parse("2026-01-01T23:15:00Z"), { timeZone: "Europe/Riga", locale: "en-GB" }), {
  time: "1:15", date: "Friday, 2 January 2026", shortDate: "2 Jan 2026", dateTime: "2026-01-01T23:15:00.000Z",
});
assert.equal(messageTimeLabels(Date.parse("2026-01-01T23:15:00Z"), { timeZone: "Europe/Riga", locale: "en-GB", now: Date.parse("2026-06-01T00:00:00Z") })?.shortDate, "2 Jan");
assert.equal(messageTimeLabels(Number.NaN, context), null);
console.log("chatTimeModel: 12 assertions passed");
