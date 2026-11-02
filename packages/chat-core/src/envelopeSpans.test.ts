import { hidden, visible, opening, eligibleDocuments } from "./envelopeSpans.fixtures.testdata";
import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { parseDomiosAttributes } from "./domios";
import { rawEnvelopeExclusions } from "./envelopeSpans";


test("envelope exclusions preserve independent source spans and never stitch tag fragments", () => {
  const frame = `${opening}\n${hidden}\n</domios>`;
  const source = visible + "\n" + frame + "\n" + visible + "\n" + frame + "\n" + visible;
  const spans = rawEnvelopeExclusions(source);
  assert.deepEqual(spans.map(({ start, end }) => source.slice(start, end)), [frame, frame]);
  assert.deepEqual(eligibleDocuments(source), Array(3).fill("/legitimate-direct.md"));
  const broken = '<ds-document src="/before' + frame + 'after.md" />';
  assert.deepEqual(eligibleDocuments(broken), []);
  assert.deepEqual(rawEnvelopeExclusions(visible), []);
  assert.deepEqual(eligibleDocuments(visible), ["/legitimate-direct.md"]);
  const truncated = `${visible}\n${frame}\n${visible}\n<domios from="tab">\n${hidden}`;
  assert.deepEqual(eligibleDocuments(truncated), Array(2).fill("/legitimate-direct.md"));
  assert.deepEqual(rawEnvelopeExclusions(truncated).map(({ complete }) => complete), [true, false]);
});

test("envelope prefix truncation and malformed nested paste closers stay excluded", () => {
  for (const kind of ["domios", "pasted_content"]) {
    for (let length = 1; length <= kind.length + 1; length++) {
      const source = `<${kind}`.slice(0, length);
      assert.deepEqual(rawEnvelopeExclusions(source), [{ start: 0, end: source.length, kind: length === 1 ? "domios" : kind, complete: false, malformed: false }]);
    }
  }
  const source = `${opening}<pasted_content id="x">\n</domios>\n</pasted_content id="wrong">\n${hidden}`;
  assert.equal(rawEnvelopeExclusions(source)[0]?.complete, false);
  assert.deepEqual(eligibleDocuments(source), []);
  assert.deepEqual(parseDomiosAttributes(' id="a&amp;b"'), { id: "a&b" });
  assert.equal(parseDomiosAttributes(' id="a" id="b"'), null);
});

test("envelope shared host corpus cannot grant raw sender authority", () => {
  const corpus = JSON.parse(readFileSync(new URL("../../../tests/fixtures/message-envelope.json", import.meta.url), "utf8")) as { name: string; raw: string }[];
  for (const fixture of corpus) {
    const spans = rawEnvelopeExclusions(fixture.raw);
    for (let index = 0; index < spans.length; index++) {
      const span = spans[index];
      assert.ok(span.start >= 0 && span.end > span.start && span.end <= fixture.raw.length, fixture.name);
      assert.ok(index === 0 || spans[index - 1].end <= span.start, fixture.name);
      assert.deepEqual(Object.keys(span).sort(), ["complete", "end", "kind", "malformed", "start"]);
    }
  }
});
