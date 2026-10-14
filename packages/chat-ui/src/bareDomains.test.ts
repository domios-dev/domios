import assert from "node:assert/strict";
import { matchBareDomains } from "./bareDomains";

const cases: Array<[string, string[]]> = [
  ["something.com", ["something.com"]],
  ["something.com/a/b", ["something.com/a/b"]],
  ["sub.domain.dev/x?y=1", ["sub.domain.dev/x?y=1"]],
  ["EXAMPLE.NET", ["EXAMPLE.NET"]],
  ["example.ai example.app example.io example.co example.me example.so example.sh example.gg example.ru example.de example.uk", ["example.ai", "example.app", "example.io", "example.co", "example.me", "example.so", "example.sh", "example.gg", "example.ru", "example.de", "example.uk"]],
  ["(something.com/a).", ["something.com/a"]],
  ["example.com/a(b)", ["example.com/a(b)"]],
  ["example.com/a(b))]!?", ["example.com/a(b)"]],
  ["something.com.,;:!?", ["something.com"]],
  ["mailto:owner@example.com", ["mailto:owner@example.com"]],
  ["file.ts main.rs README.md package.json v1.2.3 e.g. i.e. 1.14.7 foo.bar()", []],
  ["style.css app.js archive.zip settings.yaml module.py component.tsx file.sh build.sh", []],
  ["unknown.notatld foo.dev() /path/example.com owner@example.com", []],
  ["https://example.com http://example.org www.example.net", ["www.example.net"]],
  ["example.com:8080/a", ["example.com:8080/a"]],
];
for (const [source, expected] of cases) {
  const actual = matchBareDomains(source);
  assert.deepEqual(actual.map((match) => match.text), expected, source);
  for (const match of actual) {
    assert.equal(source.slice(match.start, match.end), match.text);
    assert.equal(match.url, match.text.startsWith("mailto:") ? match.text : `https://${match.text}`);
  }
}
console.log(`bareDomains: ${cases.length} tables passed`);
