import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { DsBlockScanner, parseDsBlocks, serializeDsBlock, type DsBlock, type DsLifecycle } from "./dsBlocks";
import { committed, blocks, diagrams } from "./dsBlocks.assertions.testdata";
interface ExpectedBlock { block: DsBlock; start: number; end: number; byteStart: number; byteEnd: number }
interface ExpectedDiagram { text: string; start: number; end: number; byteStart: number; byteEnd: number }
interface Fixture { name: string; source: string; trusted: boolean; lifecycle: Exclude<DsLifecycle, "streaming">; expectedBlocks: ExpectedBlock[]; expectedDiagrams: ExpectedDiagram[] }
const corpus = JSON.parse(readFileSync(new URL("./dsBlocks.fixtures.json", import.meta.url), "utf8")) as { schemaVersion: number; fixtures: Fixture[] };
const authority = (fixture: Fixture) => ({ id: "fixture", trusted: fixture.trusted });
assert.equal(corpus.schemaVersion, 1);
assert.ok(corpus.fixtures.length > 60);

export function registerDsFixtures(html: boolean) {
for (const fixture of corpus.fixtures.filter(fixture => html === fixture.source.includes("<ds-html"))) {
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
      const prefixView = JSON.stringify(scanner.segments);
      const branch = scanner.fork();
      branch.append(fixture.source.slice(split));
      assert.deepEqual(committed(branch.finish(fixture.lifecycle)), committed(whole), `isolated fork split ${split}`);
      scanner.fork().finish("ended");
      scanner.fork().finish("cancelled");
      assert.equal(scanner.text, fixture.source.slice(0, split));
      assert.equal(scanner.state, "streaming");
      assert.equal(JSON.stringify(scanner.segments), prefixView, `speculative fork mutated committed view at ${split}`);
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

}
