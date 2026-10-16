import assert from "node:assert/strict";
import test from "node:test";
import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import { createMarkdownParser, parsedMarkdown, remarkCachedParse } from "./markdownParse";
import { remarkBareDomains } from "./remarkBareDomains";

test("unchanged content shares parsing across rows and remounts; revisions invalidate", () => {
  let parses = 0;
  const parse = createMarkdownParser((source) => { parses++; return { source }; }, 32, 3);
  const first = parse("first");
  assert.equal(parse("first"), first);
  assert.notEqual(parse("first!"), first);
  assert.equal(parse("first"), first);
  assert.equal(parses, 2);
});

test("cache refreshes hits and bounds source characters, entries and oversized rows", () => {
  let parses = 0;
  const parse = createMarkdownParser((source) => ({ source, n: ++parses }), 12, 2);
  const a = parse("aaaa");
  parse("bbbb");
  assert.equal(parse("aaaa"), a);
  parse("cccc");
  assert.equal(parse("aaaa"), a);
  const b = parse("bbbb");
  assert.equal(b.n, 4);
  parse("123456789");
  assert.notEqual(parse("aaaa"), a);
  assert.notEqual(parse("oversized text"), parse("oversized text"));
});

test("transformers cannot mutate cached syntax or contaminate another render", () => {
  const source = "See example.com\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n[reference][r]\n\n[r]: https://example.org";
  const processor = unified().use(remarkParse).use(remarkGfm).use(remarkCachedParse).use(remarkBareDomains);
  const pristine = processor.parse(source);
  const first = processor.runSync(processor.parse(source));
  assert.notDeepEqual(first, pristine);
  assert.deepEqual(processor.parse(source), pristine);
  assert.deepEqual(processor.runSync(processor.parse(source)), first);
});

test("streaming revisions preserve full GFM semantics including late reference definitions", () => {
  const uncached = unified().use(remarkParse).use(remarkGfm);
  const source = "[guide][ref]\n\n```ts\nconst x = 1;\n```\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n<em>done</em>\n\n[ref]: https://example.com";
  for (let end = 1; end <= source.length; end++) {
    const revision = source.slice(0, end);
    assert.deepEqual(parsedMarkdown(revision), uncached.parse(revision));
  }
  assert.deepEqual(parsedMarkdown(source), uncached.parse(source));
});
