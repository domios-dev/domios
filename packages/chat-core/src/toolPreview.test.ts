import { buildToolPreview } from "./toolPreview";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

function eq(actual: unknown, expected: unknown, label: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${label}: expected ${e}, got ${a}`);
}

{
  const preview = buildToolPreview("line1\nline2\nline3");
  eq(preview.lines, ["line2", "line3"], "uses tail lines");
  eq(preview.hiddenLines, 1, "counts hidden lines");
  eq(preview.totalLines, 3, "counts total lines");
}

{
  const preview = buildToolPreview(" \n\u001b[31merror\u001b[0m\n\nok\n", 2);
  eq(preview.lines, ["error", "ok"], "strips ansi and empty lines");
}

{
  const preview = buildToolPreview("a\nb", 0);
  assert(preview.lines.length === 0, "zero max lines yields no lines");
  eq(preview.hiddenLines, 2, "all lines hidden when max lines zero");
}

console.log("ok - toolPreview tail summary");
