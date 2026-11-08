import assert from "node:assert/strict";
import { chatDaySeparators, messageTimeLabels, showChatTimeGutter, chatHorizontalInsets } from "./chatTimeModel";

const context = { now: Date.parse("2026-03-09T04:30:00Z"), timeZone: "America/New_York", locale: "en-GB", labels: { today: "current-day", yesterday: "previous-day" } };
assert.equal(showChatTimeGutter(1007, 28, 896), false);
assert.equal(showChatTimeGutter(1008, 28, 896), true);
assert.equal(showChatTimeGutter(1200, 28, 896), true);
assert.equal(showChatTimeGutter(1008, 80, 896), false);
assert.equal(showChatTimeGutter(1112, 80, 896), true);
assert.equal(showChatTimeGutter(Number.NaN, 28, 896), false);
assert.deepEqual(chatHorizontalInsets(1007, 20, 28, 896), { composerInset: 20, contentInset: 20, timeGutter: 0 });
assert.deepEqual(chatHorizontalInsets(1008, 20, 28, 896), { composerInset: 20, contentInset: 20, timeGutter: 36 });
assert.deepEqual(chatHorizontalInsets(1700, 402, 34, 896), { composerInset: 20, contentInset: 20, timeGutter: 42 });
for (const composerInset of [0, 16, 20, 52, 402]) {
  for (const timestampWidth of [0, 28, 80]) {
    const insets = chatHorizontalInsets(1700, composerInset, timestampWidth, 896);
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
const local = Date.parse("2026-03-09T04:30:00Z");
const undated = (index: number) => index < 2;
assert.deepEqual([...chatDaySeparators([null, null, local], context, undated)], [], "a dated row after undated messages opens no day");
assert.deepEqual([...chatDaySeparators([null, null, Date.parse("2026-03-08T05:30:00Z"), local], context, undated)],
  [[3, "current-day"]], "a later known change of day still opens");
assert.deepEqual([...chatDaySeparators([null, null, local], context)], [[2, "current-day"]], "undated notices do not hide the first day");
assert.deepEqual([...chatDaySeparators([local, null, local], context, () => true)], [[0, "current-day"]], "nor a second group on its known day");
assert.deepEqual([...chatDaySeparators([Date.parse("2026-03-08T05:30:00Z"), null, local], context, () => true)],
  [[0, "previous-day"], [2, "current-day"]], "a row on a really different day opens it");
assert.deepEqual([...chatDaySeparators([local], context, () => false, true)], [],
  "a lone send row while its transcript reattaches opens no day");
assert.deepEqual([...chatDaySeparators([Date.parse("2026-03-08T05:30:00Z"), local], context, () => false, true)],
  [[1, "current-day"]], "a known change of day inside a partial transcript still opens");
assert.deepEqual(messageTimeLabels(Date.parse("2026-01-01T23:15:00Z"), { timeZone: "Europe/Riga", locale: "en-GB" }), {
  time: "1:15", date: "Friday, 2 January 2026", shortDate: "2 Jan 2026", dateTime: "2026-01-01T23:15:00.000Z",
});
assert.equal(messageTimeLabels(Date.parse("2026-01-01T23:15:00Z"), { timeZone: "Europe/Riga", locale: "en-GB", now: Date.parse("2026-06-01T00:00:00Z") })?.shortDate, "2 Jan");
assert.equal(messageTimeLabels(Number.NaN, context), null);
console.log("chatTimeModel: 15 assertions passed");
