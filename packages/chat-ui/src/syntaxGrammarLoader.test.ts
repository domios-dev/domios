import assert from "node:assert/strict";
import test from "node:test";
import { createGrammarLoader } from "./syntaxGrammarLoader";

test("concurrent grammar requests share one import and its successful result", async () => {
  let imports = 0;
  const loader = createGrammarLoader(async () => { imports += 1; }, () => assert.fail("success must not log failure"));
  const first = loader();
  assert.equal(loader(), first);
  assert.equal(await first, "loaded");
  assert.equal(await loader(), "loaded");
  assert.equal(imports, 1);
});

test("rejected grammar imports settle as plain fallback and log once", async () => {
  const error = new Error("missing chunk");
  const logged: unknown[] = [];
  let imports = 0;
  const loader = createGrammarLoader(() => { imports += 1; throw error; }, (failure) => logged.push(failure));
  assert.equal(await loader(), "failed");
  assert.equal(await loader(), "failed");
  assert.equal(imports, 1);
  assert.deepEqual(logged, [error]);
});
