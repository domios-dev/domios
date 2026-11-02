import { committed, blocks, diagrams } from "./dsBlocks.assertions.testdata";
import assert from "node:assert/strict";
import test from "node:test";
import { DsBlockScanner, parseDsBlocks, serializeDsBlock, type DsBlock } from "./dsBlocks";

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

test("dsBlocks tool and paste wrappers suppress every prefix of nested HTML and agent effects", () => {
  for (const wrapper of ["tool_result", "tool_output", "transcript", "pasted_content"]) {
    const html = '<ds-html><input/><svg><path/></svg><script>const outer="</ds-html>";</script><button>Local action</button></ds-html>';
    const excerpt = `<${wrapper}>\n${html}\n<ds-agent tab="deadbeef" />\n</${wrapper}>`;
    const scanner = new DsBlockScanner({ id: wrapper, trusted: true });
    for (const [index, unit] of [...excerpt].entries()) {
      scanner.append(unit);
      assert.equal(committed(scanner.segments).length, 0, `${wrapper} prefix ${index + 1}: nested effects`);
    }
    scanner.append(`\n${html}\n<ds-agent tab="outside" />`);
    assert.deepEqual(blocks(scanner.segments).map(({ block }) => block.kind), ["html", "agent"]);
    assert.deepEqual(blocks(scanner.segments).at(-1)?.block, { kind: "agent", tab: "outside" });
  }
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


test("dsBlocks fork views share committed identities and isolate subsequent updates in either branch", () => {
  const source = Array.from({ length: 128 }, (_, index) => `<ds-plugin id="plugin-${index}" />`).join("\n");
  const scanner = new DsBlockScanner({ id: "fork", trusted: true });
  scanner.append(source);
  const view = scanner.segments;
  const first = view[0];
  const length = view.length;
  assert.ok(Array.isArray(view));
  assert.equal(Object.keys(view).length, length);
  assert.equal(Object.getOwnPropertyDescriptor(view, "length")?.value, length);
  assert.deepEqual(view.slice(0, 3), [view[0], view[1], view[2]]);
  assert.equal(JSON.parse(JSON.stringify(view)).length, length);
  assert.equal(Reflect.set(view, "0", { type: "malformed" }), false);
  const abandoned = scanner.fork();
  abandoned.append('\n<ds-html><input/></ds-html>');
  const actual = scanner.fork();
  actual.append('\nactual suffix');
  assert.equal(scanner.text, source);
  assert.equal(view.length, length);
  assert.equal(actual.segments[0], first);
  assert.equal(abandoned.segments[0], first);
  const forkSource = actual.text;
  scanner.append('\nother source suffix');
  assert.equal(actual.text, forkSource);
  assert.equal(actual.segments[0], first);
});


test("dsBlocks forked pending Markdown fence metadata remains isolated when the original completes", () => {
  const scanner = new DsBlockScanner({ id: "fence-fork", trusted: true });
  scanner.append('```\ngraph TD\n A-->B\n');
  const branch = scanner.fork();
  const before = JSON.stringify(branch.segments);
  scanner.append('```\n');
  assert.equal(JSON.stringify(branch.segments), before);
  branch.finish("cancelled");
  const segment = branch.segments[0];
  assert.equal(segment.type, "markdown");
  if (segment.type === "markdown") assert.equal(segment.fences.length, 0);
});
