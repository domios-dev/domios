import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import test from "node:test";
import { resolveBash, runTsUnitTests } from "./run-ts-unit-tests.mjs";

test("uses bash from PATH outside Windows", () => {
  assert.equal(resolveBash({ platform: "linux" }), "bash");
});

test("uses Git Bash from Program Files on Windows", () => {
  const bash = "C:\\Program Files\\Git\\bin\\bash.exe";
  assert.equal(
    resolveBash({ platform: "win32", env: { ProgramFiles: "C:\\Program Files" }, exists: (file) => file === bash }),
    bash,
  );
});

test("accepts an explicit Windows Bash path", () => {
  const bash = "D:\\tools\\bash.exe";
  assert.equal(resolveBash({ platform: "win32", env: { CODETERM_BASH: bash }, exists: () => true }), bash);
});

test("does not attempt a missing Windows Bash executable", () => {
  assert.equal(resolveBash({ platform: "win32", env: {}, exists: () => false }), undefined);
});

test("runs the existing shell runner through the resolved Windows Bash", async () => {
  const bash = "C:\\Program Files\\Git\\bin\\bash.exe";
  const child = new EventEmitter();
  const code = runTsUnitTests({
    platform: "win32",
    env: { ProgramFiles: "C:\\Program Files" },
    exists: (file) => file === bash,
    spawnProcess(command, args, options) {
      assert.equal(command, bash);
      assert.deepEqual(args, ["scripts/ts-unit-tests.sh"]);
      assert.equal(options.stdio, "inherit");
      queueMicrotask(() => child.emit("exit", 0, null));
      return child;
    },
  });
  assert.equal(await code, 0);
});
