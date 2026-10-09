import assert from "node:assert/strict";
import { test } from "node:test";
import { createComposerAdmission } from "./composerAdmission";

test("an unresolved draft has one admission regardless of repeated submits", () => {
  const admission = createComposerAdmission();
  const release = admission.claim(1);
  assert(release);
  for (let attempt = 0; attempt < 6; attempt++) assert.equal(admission.claim(1), null);
  release();
  assert(admission.claim(1));
});

test("a newer draft can be submitted while the previous revision is pending", () => {
  const admission = createComposerAdmission();
  const first = admission.claim(1);
  const second = admission.claim(2);
  assert(first);
  assert(second);
  first();
  assert.equal(admission.claim(2), null);
  second();
  assert(admission.claim(2));
});
