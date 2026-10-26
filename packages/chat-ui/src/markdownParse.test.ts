import assert from "node:assert/strict";
import test from "node:test";
import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import { createIncrementalMarkdownParser, createMarkdownParser, parsedMarkdown, remarkCachedParse } from "./markdownParse";
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

const STREAM_CORPUS = [
  "# Plan\n\nIntro paragraph with **bold**, _em_, ~~gone~~ and `code`.\nSecond line\n===\n\nSetext two\n---\n",
  "- one\n- two\n\n- loose three\n  continued\n\n  second para\n- [ ] task\n- [x] done\n\n1. first\n2. second\n\n10) other\n\n+ plus list\n* star list\n-\n\nafter list\n",
  "1. Step\n   ```bash\n   cmd --flag\n\n   more\n   ```\n2. Next\n\n```ts\nconst a = 1;\n\nconst b = 2;\n```\n\n~~~\ntilde\n```\nstill code\n~~~\n\n    indented\n\n    code\n\ntext\n",
  "> quote\nlazy\n\n> ```\n> fenced in quote\n>\n> x\n> ```\n\n> second\n\n| A | B |\n| - | :-: |\n| 1 | 2 |\nrow without pipe\n\nparagraph\n| h |\n| - |\n| c |\n",
  "<div>\nhtml\n</div>\n\n<pre>\n\nkept\n\n</pre>\n\n<!--\n\ncomment\n\n-->\n\n<custom-tag>\ninline\n\n***\n\n\tTabbed\n\n  - indented list\n\n   ```\n   indented fence\n```\n",
  "para\n#x\n\n1. a\n\n2. b\n\n- c\n\nd\n- e\n\n> q\n>\n> r\n\n# h\n\n#\n\n| t |\n\n| u |\n| - |\n",
  "    code\n\n2. b\n2\n\n1.\n\n     deep\n  -\n\n- a\n\n      code\n\n- b\n  - nested\n\n    lazy\n3) c\n\n10. x\n11. *y*\n",
  "Line one\r\nline two\r\n\r\n- crlf item\r\n\r\n```\r\ncode\r\n```\r\n\r\nhttps://example.com and www.example.org\n\nEnd *open emphasis\n\n```\nunclosed fence\n\nstill inside",
];

test("streamed revisions parse only the open tail and stay identical to a full parse", () => {
  const full = unified().use(remarkParse).use(remarkGfm);
  const joined = STREAM_CORPUS.join("\n");
  for (const document of [...STREAM_CORPUS, joined]) {
    const parse = createIncrementalMarkdownParser((source) => full.parse(source));
    // Each corpus entry covers every boundary; the joined document re-checks their seams at a prime stride.
    const stride = document === joined ? 7 : 1;
    for (let end = 0; end <= document.length; end = end === document.length ? end + 1 : Math.min(document.length, end + stride)) {
      const revision = document.slice(0, end);
      assert.deepEqual(parse(revision), full.parse(revision), JSON.stringify(revision));
    }
  }
});

test("random stream chunks keep full-parse semantics and reuse finished blocks", () => {
  const full = unified().use(remarkParse).use(remarkGfm);
  const document = STREAM_CORPUS.join("\n\n");
  let seed = 7;
  const random = () => (seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648;
  for (let run = 0; run < 8; run++) {
    const parsed: number[] = [];
    const parse = createIncrementalMarkdownParser((source) => { parsed.push(source.length); return full.parse(source); });
    let previous: ReturnType<typeof parse> | null = null;
    for (let end = 0; end < document.length;) {
      end = Math.min(document.length, end + 1 + Math.floor(random() * 40));
      const tree = parse(document.slice(0, end));
      assert.deepEqual(tree, full.parse(document.slice(0, end)));
      if (previous && tree.children.length > 2 && previous.children.length > 2) assert.equal(tree.children[0], previous.children[0]);
      previous = tree;
    }
    assert.ok(parsed.at(-1)! < document.length / 4, `last delta parsed ${parsed.at(-1)} of ${document.length} chars`);
  }
});

test("reference definitions anywhere force a full parse so earlier references resolve", () => {
  const full = unified().use(remarkParse).use(remarkGfm);
  const parse = createIncrementalMarkdownParser((source) => full.parse(source));
  const document = "See [guide][g] and note[^n].\n\nMore text.\n\n[g]: https://example.com\n\n[^n]: Footnote.";
  for (let end = 0; end <= document.length; end++) assert.deepEqual(parse(document.slice(0, end)), full.parse(document.slice(0, end)));
});

test("an unchanged source returns the same tree and separate streams stay independent", () => {
  const full = unified().use(remarkParse).use(remarkGfm);
  const parse = createIncrementalMarkdownParser((source) => full.parse(source), 2);
  const a = parse("alpha\n\nbeta");
  assert.equal(parse("alpha\n\nbeta"), a);
  const b = parse("other\n\nstream");
  assert.deepEqual(parse("alpha\n\nbeta gamma"), full.parse("alpha\n\nbeta gamma"));
  assert.deepEqual(parse("other\n\nstream two"), full.parse("other\n\nstream two"));
  assert.notEqual(parse("other\n\nstream two"), b);
});

test("a long streamed list re-parses only its last item and keeps list semantics", () => {
  const full = unified().use(remarkParse).use(remarkGfm);
  const items = Array.from({ length: 300 }, (_, index) => `${index + 1}. fact ${index + 1} about computing${index === 150 ? "\n\n     with a second paragraph" : ""}`);
  const document = `Intro\n\n${items.join("\n")}\n\nDone.`;
  const parsed: number[] = [];
  const parse = createIncrementalMarkdownParser((source) => { parsed.push(source.length); return full.parse(source); });
  for (let end = 0; end < document.length; end = Math.min(document.length, end + 25)) {
    const revision = document.slice(0, end + 25);
    const tree = parse(revision);
    if (end % 1000 < 25 || end + 25 >= document.length) assert.deepEqual(tree, full.parse(revision));
  }
  assert.ok(Math.max(...parsed.slice(parsed.length / 2)) < 120, `largest late re-parse ${Math.max(...parsed.slice(parsed.length / 2))} chars`);
});

test("long finished messages stay cached instead of re-parsing on every render", () => {
  let parses = 0;
  const parse = createMarkdownParser((source) => { parses++; return { source }; });
  const long = "A long paragraph of agent output.\n\n".repeat(3000);
  assert.ok(long.length > 100_000);
  assert.equal(parse(long), parse(long));
  assert.equal(parses, 1);
});
