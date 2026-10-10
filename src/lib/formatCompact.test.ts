import assert from "node:assert/strict";
import { formatCompact, formatExact } from "./formatCompact";

const cases: Array<[number, string]> = [
  [0, "0"], [38, "38"], [999, "999"], [999.6, "1k"], [1_000, "1k"], [1_049, "1k"], [1_100, "1.1k"], [9_900, "9.9k"],
  [9_960, "10k"], [10_000, "10k"], [12_345, "12k"], [999_000, "999k"], [999_600, "1M"], [1_000_000, "1M"],
  [1_234_567, "1.2M"], [12_500_000, "13M"], [2_000_000_000, "2B"], [-1_500, "-1.5k"], [-38, "-38"],
];
for (const [value, expected] of cases) assert.equal(formatCompact(value), expected, `formatCompact(${value})`);
assert.equal(formatCompact(Number.NaN), "—");
assert.equal(formatExact(12_345), "12,345");
console.log("formatCompact checks passed");
