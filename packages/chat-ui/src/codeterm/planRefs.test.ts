import assert from "node:assert/strict";
import { extractPlanRefs, linkifyPlanRefs, hasPlanRefs } from "./planRefs";
import { referenceMarkers, referenceMarkerText } from "./referenceMarkers";
import { canonicalMentions } from "./canonicalMentions";

const task = referenceMarkerText("task", "task_a", "pl_a");
const section = referenceMarkerText("section", "sec_a", "pl_b");
const plan = referenceMarkerText("plan", "pl_a");
const tab = referenceMarkerText("tab", "tab_a");
const text = `Open ${plan}, then ${task} and ${section}. ${tab}`;
assert.deepEqual(extractPlanRefs(text).map(ref => [ref.targetKind, ref.planId, ref.targetId]), [
  ["plan", "pl_a", undefined], ["task", "pl_a", "task_a"], ["section", "pl_b", "sec_a"],
]);
assert.equal(linkifyPlanRefs(text).map(segment => segment.value).join(""), text);
assert.equal(extractPlanRefs(`${task} ${task}`).length, 1);
assert.deepEqual(canonicalMentions(tab).map(ref => [ref.kind, ref.id]), [["tab", "tab_a"]]);
assert.deepEqual(canonicalMentions(referenceMarkerText("task", "task_a")).map(ref => ref.id), ["task_a"]);
for (const plain of ["pl_a task_a sec_a tab_a COD-12", "@Name (tab tab_a) @Plan (plan pl_a)",
  `domios task show pl_a task_a`, `https://example.test/pl_a/task_a`,
  `\`${task}\``, `\`\`${task}\`\``, `\`\`\`text\n${task}\n\`\`\``, `~~~text\n${task}\n~~~`, `\`\`\`text\n${task}`]) {
  assert.equal(hasPlanRefs(plain), false, plain);
  assert.deepEqual(canonicalMentions(plain), [], plain);
}
for (const invalid of ['<domios type="task" id="task_a" id="task_b" plan="pl_a" />',
  '<domios type="section" id="sec_a" />', '<domios from="worker">', '<domios type="attachment" kind="file" path="/a" />']) {
  assert.deepEqual(referenceMarkers(invalid), []);
}
assert.equal(referenceMarkers(referenceMarkerText("tab", 'a&"b'))[0].id, 'a&"b');
console.log("explicit reference parsing checks passed");
