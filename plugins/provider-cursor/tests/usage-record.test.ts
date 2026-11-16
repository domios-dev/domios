import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
const context=vm.createContext({module:{exports:{}},exports:{},host:{}});
context.exports=context.module.exports;
vm.runInContext(fs.readFileSync(new URL("../plugin.js",import.meta.url),"utf8"),context);
const plugin=context.module.exports.default;
const cases=JSON.parse(fs.readFileSync(new URL("../../provider-shared/tests/fixtures/usage-records.json",import.meta.url),"utf8"));
let count=0;
for (const fixture of cases.filter((row:{provider:string})=>row.provider === "cursor")) {
  assert.deepEqual(JSON.parse(JSON.stringify(plugin.parseUsageRecord(fixture.record,123))),fixture.expected);
  count++;
}
assert.ok(count>0);
assert.equal(plugin.usageAccounting(),"unreported");
console.log(`PASS provider usage normalization: ${count} captured records`);
