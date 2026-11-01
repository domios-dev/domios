import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

const source = fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8");
const manifest = JSON.parse(fs.readFileSync(new URL("../plugin.json", import.meta.url), "utf8"));
assert.equal(manifest.rule.path, "~/.claude/rules/codeterm.md");

interface RulePlugin {
  installBridgeRule(payload: string): { ok: boolean };
  uninstallBridgeRule(): { ok: boolean };
}

for (const declaration of [null, manifest, { rule: { path: "~/custom/rule.md" } }]) {
  const files = new Map<string, string>();
  const expanded: string[] = [];
  const context = {
    host: {
      manifestJson: () => JSON.stringify(declaration),
      expandHome: (path: string) => {
        expanded.push(path);
        return path.replace("~/", "/home/fixture/");
      },
      writeFile: (path: string, payload: string) => {
        files.set(path, payload);
        return true;
      },
      fileExists: (path: string) => files.has(path),
      removeFile: (path: string) => files.delete(path),
      log: () => {},
    },
    module: { exports: {} as { default: RulePlugin } },
    exports: {},
  };
  new vm.Script(source).runInNewContext(context);
  const plugin = context.module.exports.default;
  const expected = declaration?.rule?.path === "~/custom/rule.md"
    ? "~/custom/rule.md" : "~/.claude/rules/codeterm.md";
  assert.equal(plugin.installBridgeRule("opaque payload").ok, true);
  assert.deepEqual([...files], [[expected.replace("~/", "/home/fixture/"), "opaque payload"]]);
  assert.equal(plugin.uninstallBridgeRule().ok, true);
  assert.equal(files.size, 0);
  assert.deepEqual(expanded, [expected, expected]);
}

console.log("bridge-rule: 3 manifest/default destination cases passed");
