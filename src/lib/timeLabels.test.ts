import { strict as assert } from "node:assert";
import { test } from "node:test";
import { formatAge, formatDuration, formatSpan, formatUntil } from "./timeLabels";

const S = 1000;
const M = 60 * S;
const H = 60 * M;
const D = 24 * H;
const NOW = 10_000 * D;

test("a span keeps only its largest whole unit", () => {
  assert.deepEqual(
    [5 * S, 2 * M, 2 * H, 2 * D, 45 * D, 400 * D].map(formatSpan),
    ["5s", "2m", "2h", "2d", "1mo", "1y"],
  );
  assert.equal(formatSpan(-5 * S), "0s");
});

test("a duration keeps two units so short runs stay precise", () => {
  assert.deepEqual(
    [45 * S, 3 * M, 3 * M + 10 * S, H, H + 5 * M, 2 * D + 3 * H].map(formatDuration),
    ["45s", "3m 0s", "3m 10s", "1h", "1h 5m", "2d 3h"],
  );
});

test("an age always says ago, and the last minute is just now", () => {
  assert.equal(formatAge(NOW - 30 * S, NOW), "just now");
  assert.equal(formatAge(NOW - 2 * M, NOW), "2m ago");
  assert.equal(formatAge(NOW - 7 * D, NOW), "7d ago");
});

test("a future time always says in, and a passed one reads as an age", () => {
  assert.equal(formatUntil(NOW + 30 * S, NOW), "in <1m");
  assert.equal(formatUntil(NOW + 2 * H, NOW), "in 2h");
  assert.equal(formatUntil(NOW - 5 * M, NOW), "5m ago");
});

test("age, duration and future never read alike for the same span", () => {
  const span = 5 * M;
  const labels = new Set([formatAge(NOW - span, NOW), formatDuration(span), formatUntil(NOW + span, NOW)]);
  assert.equal(labels.size, 3);
});
