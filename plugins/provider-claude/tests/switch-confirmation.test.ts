import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const context = vm.createContext({ module: { exports: {} }, exports: {}, host: {} });
vm.runInContext(fs.readFileSync(path.join(root, "plugin.js"), "utf8"), context);
const plugin = context.module.exports.default;
const parse = (screen: string, selection: string) => plugin.parseSwitchConfirmationPrompt(screen, selection);
let cases = 0;
for (const [name, selection] of [
  ["sonnet-confirmation.txt", "sonnet"],
  ["cached-sonnet-confirmation.txt", "sonnet"],
  ["opus-confirmation.txt", "opus"],
]) {
  const screen = fs.readFileSync(path.join(root, "tests/fixtures/model-switch", name), "utf8");
  for (const frame of [screen, screen.replaceAll("\n", "\r\n")]) {
    const prompt = parse(frame, selection);
    assert.ok(prompt);
    assert.deepEqual(Array.from(prompt.options, (option: string[]) => option[1]), ["1", "2"]);
    assert.equal(parse(frame, "foreign"), null);
    assert.equal(parse(`${frame}\n❯ `, selection), null);
    assert.equal(parse(frame.replace("❯ 1.", "  1."), selection), null);
    assert.equal(parse(frame.replace("❯ 1.", "  1.").replace("  2.", "❯ 2."), selection), null);
    assert.equal(parse(`${frame}\nopaque unrelated frame`, selection), null);
    const rewritten = frame.replace(/[^\r\n]*\?/, "Opaque header?")
      .replace(/(1\. )[^\r\n]+/, `$1opaque ${selection}`)
      .replace(/(2\. )[^\r\n]+/, "$1opaque alternate");
    assert.ok(parse(rewritten, selection));
    cases += 8;
  }
}
const synthetic = "Opaque header?\nOpaque context\n❯ 1. opaque high\n  2. opaque alternate";
assert.ok(parse(synthetic, "high"));
assert.equal(parse("Opaque header?\n❯ 1. opaque high\n  2. opaque alternate", "high"), null);
assert.equal(parse(synthetic.replace("2. opaque alternate", "2. opaque high"), "high"), null);
console.log(`${cases + 3} switch confirmation classifier checks passed`);
