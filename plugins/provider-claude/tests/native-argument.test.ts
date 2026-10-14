import assert from "node:assert/strict";
import { quoteNativeArgument, quoteWindowsArgument } from "../../provider-shared/nativeArgument";

function parseWindowsCommandLine(command: string): string[] {
  const args: string[] = [];
  let index = 0;
  while (index < command.length) {
    while (/[ \t]/.test(command[index] ?? "") && index < command.length) index++;
    if (index === command.length) break;
    let value = "";
    let quoted = false;
    while (index < command.length && (quoted || !/[ \t]/.test(command[index]))) {
      let slashes = 0;
      while (command[index] === "\\") { slashes++; index++; }
      if (command[index] === '"') {
        value += "\\".repeat(Math.floor(slashes / 2));
        if (slashes % 2) value += '"';
        else quoted = !quoted;
        index++;
      } else {
        value += "\\".repeat(slashes);
        if (index < command.length) value += command[index++];
      }
    }
    assert.equal(quoted, false);
    args.push(value);
  }
  return args;
}

const frame = '<domios from="system" context="assigned-task">opaque body</domios>';
const fixtures = [frame, "", "plain", "two words", "slash\\", 'slash\\"quote', '\\\\"',
  '<domios from="system" context="assigned-task">$env:VALUE `x` %PATH% \'quoted\'\n🦀 \\</domios>'];
const powershellQuote = (value: string) => `'${value.replace(/'/g, "''")}'`;
for (const value of fixtures) {
  const encoded = quoteWindowsArgument(value);
  assert.deepEqual(parseWindowsCommandLine(`before ${encoded} after`), ["before", value, "after"]);
  const expression = quoteNativeArgument(value, "windows", powershellQuote);
  const branches = /\{ ('(?:[^']|'')*') \} else \{ ('(?:[^']|'')*') \}/s.exec(expression)!;
  const decodeLiteral = (literal: string) => literal.slice(1, -1).replace(/''/g, "'");
  assert.equal(decodeLiteral(branches[1]), value);
  assert.deepEqual(parseWindowsCommandLine(decodeLiteral(branches[2])), [value]);
  assert.equal(quoteNativeArgument(value, "linux", powershellQuote), powershellQuote(value));
  assert.equal(quoteNativeArgument(value, "macos", powershellQuote), powershellQuote(value));
}
assert.notDeepEqual(parseWindowsCommandLine(`"${frame}"`), [frame]);
console.log(`${fixtures.length} Windows argument round-trip fixtures passed`);
