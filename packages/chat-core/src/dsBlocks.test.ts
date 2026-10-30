import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { DsBlockScanner, parseDsBlocks, serializeDsBlock, type DsBlock, type DsLifecycle, type DsSegment } from "./dsBlocks";
interface ExpectedBlock { block: DsBlock; start: number; end: number; byteStart: number; byteEnd: number }
interface ExpectedDiagram { text: string; start: number; end: number; byteStart: number; byteEnd: number }
interface Fixture { name: string; source: string; trusted: boolean; lifecycle: Exclude<DsLifecycle, "streaming">; expectedBlocks: ExpectedBlock[]; expectedDiagrams: ExpectedDiagram[] }
const corpus = JSON.parse(readFileSync(new URL("./dsBlocks.fixtures.json", import.meta.url), "utf8")) as { schemaVersion: number; fixtures: Fixture[] };
const authority = (fixture: Fixture) => ({ id: "fixture", trusted: fixture.trusted });
const committed = (segments: readonly DsSegment[]) => segments.filter((part) => part.type === "block" || part.type === "diagram");
const blocks = (segments: readonly DsSegment[]) => segments.flatMap((part) => part.type === "block" ? [{ block: part.block, start: part.start, end: part.end }] : []);
const diagrams = (segments: readonly DsSegment[]) => segments.flatMap((part) => part.type === "diagram" ? [{ text: part.text, start: part.start, end: part.end }] : []);
assert.equal(corpus.schemaVersion, 1);
assert.ok(corpus.fixtures.length > 60);
for (const fixture of corpus.fixtures) {
  test(`dsBlocks fixture: ${fixture.name}`, () => {
    const whole = parseDsBlocks(fixture.source, authority(fixture), fixture.lifecycle);
    assert.deepEqual(blocks(whole), fixture.expectedBlocks.map(({ block, start, end }) => ({ block, start, end })));
    assert.deepEqual(diagrams(whole), fixture.expectedDiagrams.map(({ text, start, end }) => ({ text, start, end })));
    for (const expected of [...fixture.expectedBlocks, ...fixture.expectedDiagrams]) {
      assert.equal(new TextEncoder().encode(fixture.source.slice(0, expected.start)).length, expected.byteStart);
      assert.equal(new TextEncoder().encode(fixture.source.slice(0, expected.end)).length, expected.byteEnd);
    }
    for (let split = 0; split <= fixture.source.length; split++) {
      const scanner = new DsBlockScanner(authority(fixture));
      scanner.append(fixture.source.slice(0, split));
      for (const part of committed(scanner.segments)) {
        assert.ok(committed(whole).some((full) => JSON.stringify(full) === JSON.stringify(part)), `premature effect at UTF-16 split ${split}`);
      }
      scanner.append(fixture.source.slice(split));
      scanner.finish(fixture.lifecycle);
      assert.deepEqual(committed(scanner.segments), committed(whole), `UTF-16 split ${split}`);
    }
    const one = new DsBlockScanner(authority(fixture));
    for (let index = 0; index < fixture.source.length; index++) one.append(fixture.source[index]);
    assert.deepEqual(committed(one.finish(fixture.lifecycle)), committed(whole), "one UTF-16 code unit schedule");
    const bytes = new TextEncoder().encode(fixture.source);
    for (let split = 0; split <= bytes.length; split++) {
      const decoder = new TextDecoder();
      const scanner = new DsBlockScanner(authority(fixture));
      scanner.append(decoder.decode(bytes.slice(0, split), { stream: true }));
      scanner.append(decoder.decode(bytes.slice(split)));
      assert.deepEqual(committed(scanner.finish(fixture.lifecycle)), committed(whole), `UTF-8 split ${split}`);
    }
    const bytewise = new DsBlockScanner(authority(fixture));
    const decoder = new TextDecoder();
    for (const byte of bytes) bytewise.append(decoder.decode(Uint8Array.of(byte), { stream: true }));
    bytewise.append(decoder.decode());
    assert.deepEqual(committed(bytewise.finish(fixture.lifecycle)), committed(whole), "one byte schedule");
    for (const expected of fixture.expectedBlocks) {
      const roundTrip = parseDsBlocks(serializeDsBlock(expected.block));
      assert.deepEqual(blocks(roundTrip).map((part) => part.block), [expected.block], "typed serialization round trip");
    }
  });
}

test("dsBlocks committed object identity survives twenty chunks and reset invalidates authority", () => {
  const scanner = new DsBlockScanner({ id: "one", trusted: true });
  scanner.append('<ds-html><input/><svg><path/></svg></ds-html>');
  const block = scanner.segments[0];
  assert.equal(block.type, "block");
  for (let index = 0; index < 20; index++) {
    scanner.append(`\n\nfollowing ${index}`);
    assert.equal(scanner.segments[0], block);
  }
  scanner.reset({ id: "two", trusted: false });
  scanner.append('<ds-html><input/></ds-html>');
  assert.equal(scanner.segments[0].type, "inert");
  assert.notEqual(scanner.segments[0].id, block.id);
});

test("dsBlocks end and cancellation never repair a missing terminator", () => {
  for (const lifecycle of ["ended", "cancelled"] as const) {
    const scanner = new DsBlockScanner({ id: "message", trusted: true });
    scanner.append('<ds-html><input/>');
    assert.equal(scanner.segments[0].type, "pending");
    scanner.finish(lifecycle);
    assert.equal(scanner.segments[0].type, "malformed");
    assert.equal(committed(scanner.segments).length, 0);
    if (lifecycle === "cancelled") { scanner.finish("ended"); assert.throws(() => scanner.append('</ds-html>')); }
    else {
      scanner.append('</ds-html>');
      assert.equal(committed(scanner.segments).length, 0);
    }
  }
});

test("dsBlocks enforces file, gallery, label and HTML bounds", () => {
  const source = Array.from({ length: 33 }, () => '<ds-page src="/a.html" />').join("\n");
  assert.equal(blocks(parseDsBlocks(source)).length, 32);
  const gallery: DsBlock = { kind: "image", sources: Array.from({ length: 33 }, (_, index) => `/${index}.png`) };
  assert.equal(blocks(parseDsBlocks(serializeDsBlock(gallery))).length, 0);
  for (const options of [["Yes", "x".repeat(81)], Array.from({ length: 9 }, (_, index) => String(index))]) {
    assert.equal(blocks(parseDsBlocks(serializeDsBlock({ kind: "choice", options }))).length, 0);
  }
  assert.equal(blocks(parseDsBlocks(`<ds-html>${"x".repeat(200_001)}</ds-html>`)).length, 0);
});
