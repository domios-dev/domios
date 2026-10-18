import assert from "node:assert/strict";
import test from "node:test";
import { roleDisplayWord } from "./roleDisplay";

test("role labels keep acronyms and title-case words", () => {
  assert.equal(roleDisplayWord("qa"), "QA");
  assert.equal(roleDisplayWord("QA"), "QA");
  assert.equal(roleDisplayWord("orchestrator"), "Orchestrator");
  assert.equal(roleDisplayWord("WORKER"), "Worker");
});
