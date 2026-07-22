import { unwrapShellCommand } from "../src/execWrapper";
import { patchEdits } from "../src/patchEdits";

function eq(actual: unknown, expected: unknown, label: string): void {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(label + ": " + JSON.stringify(actual));
  console.log("  ✓ " + label);
}

eq(
  unwrapShellCommand(String.raw`const r = await tools.shell_command({command:"codeterm agent reports --tab e3fa0754",workdir:"D:\x",timeout_ms:20000}); text(r)`),
  { command: "codeterm agent reports --tab e3fa0754", workdir: "D:\\x", timeout_ms: 20000 },
  "unwraps shell command metadata",
);
eq(
  unwrapShellCommand(String.raw`const r = await tools.shell_command({"command":"& 'C:\\Program Files\\Git\\bin\\bash.exe' -lc 'source evals/_lib/lib.sh && dc plugin update codex --json'","workdir":"D:\\Developer\\codeterm","timeout_ms":20000}); text(r)`),
  {
    command: "& 'C:\\Program Files\\Git\\bin\\bash.exe' -lc 'source evals/_lib/lib.sh && dc plugin update codex --json'",
    workdir: "D:\\Developer\\codeterm",
    timeout_ms: 20000,
  },
  "unwraps generated Windows JSON wrapper",
);
eq(unwrapShellCommand(String.raw`tools.shell_command({command:"unterminated})`), null, "rejects unbalanced wrapper");
eq(
  patchEdits("*** Begin Patch\n*** Update File: src/demo.ts\n@@\n-old\n+new\n*** End Patch"),
  [{ path: "src/demo.ts", old: "old", new: "new" }],
  "extracts patch edits",
);
